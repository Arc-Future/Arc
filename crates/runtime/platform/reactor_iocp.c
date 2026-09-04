// Windows IOCP Reactor 后端实现（RFC 009 M4）。
//
// IOCP（IO Completion Port）是 Windows 的异步 IO 机制：
//   - 创建 completion port（CreateIoCompletionPort）
//   - 将文件句柄/socket 关联到 IOCP
//   - 发起异步 IO（ReadFile/WriteFile/WSARecv/WSASend + OVERLAPPED）
//   - 轮询完成事件（GetQueuedCompletionStatusEx）
//
// 与 io_uring 的差异：
//   - IOCP 是"完成"模型（IO 完成后通知），io_uring 是"提交"模型（提交请求后轮询 CQE）
//   - IOCP 不支持批量提交（每个 IO 独立发起），但 GetQueuedCompletionStatusEx 可批量取完成
//   - IOCP 无零拷贝缓冲池注册（模拟实现：仅池化 buffer）
//
// 性能特征：
//   - 每 IO 1 syscall（ReadFile/WriteFile），无 io_uring 的批量提交优势
//   - 完成事件批量获取（GetQueuedCompletionStatusEx 一次最多 max_events）
//   - 适合高并发连接场景（IOCP 原生支持）
//
// 与 RFC 009 §3.2 对齐。

#ifdef _WIN32

#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <winsock2.h>
#include <mswsock.h>
#include <ws2tcpip.h>
#include <stdio.h>
#include <stdatomic.h>

#pragma comment(lib, "ws2_32.lib")
#pragma comment(lib, "mswsock.lib")

/* ---- IOCP Reactor 内部结构 ---- */

/* 操作类型（区分完成事件的来源） */
typedef enum {
    RT_IOCP_OP_READ     = 1,
    RT_IOCP_OP_WRITE    = 2,
    RT_IOCP_OP_ACCEPT   = 3,
    RT_IOCP_OP_CONNECT  = 4,
} RtIocpOpType;

/* 每 IO 的上下文（OVERLAPPED 扩展） */
typedef struct RtIocpOverlapped {
    OVERLAPPED  overlapped;     /* 必须在首位，IOCP 返回的指针指向此字段 */
    void*       user_data;      /* 用户数据（waker/task） */
    RtIocpOpType op_type;       /* 操作类型 */
    int32_t     fd;             /* 关联的 fd（HANDLE/SOCKET） */
    WSABUF      wsa_buf;        /* WSARecv/WSASend 用的缓冲 */
    char        accept_buf[2 * (sizeof(SOCKADDR_IN) + 16)]; /* AcceptEx 用 */
    SOCKET      accept_socket;  /* RFC 009 M2: AcceptEx 预创建的 accept socket */
    int32_t     listen_family;  /* listener 地址族（用于预创建 socket） */
    struct RtIocpOverlapped* next;  /* 复用池 free-list 链（IO 吞吐预算） */
} RtIocpOverlapped;

/* fd 类型缓存（RFC 016 IO 吞吐预算）：每 submit 的 GetFileType 是 syscall，
 * 缓存于 register/submit 首命中后消除。直映射 128 槽，fd 哈希索引。 */
#define RT_IOCP_TYPE_CACHE_SIZE 128
typedef struct {
    int32_t fd;
    uint8_t is_socket;
    uint8_t valid;
} RtIocpTypeEntry;

/* 立即完成事件 FIFO：submit 阶段合成的完成（如非阻塞 socket 无数据就绪）。
 * Reactor 不自持对 rt_net.c 的硬链（rt_io_completion_complete 定义于 rt_net.c），
 * 合成完成以事件形式入队、由 poll 原样交付——Reactor 保持可独立链接
 * （adv_async_io_throughput e2e 仅链接 rt_reactor.c）。容量按批提交上限设计，
 * 单 Reactor 单 EventLoop 访问，无需加锁。 */
#define RT_IOCP_IMMEDIATE_CAP 64
typedef struct {
    RtIoEvent items[RT_IOCP_IMMEDIATE_CAP];
    uint32_t  head;
    uint32_t  tail;
    uint32_t  count;
} RtIocpImmediateQueue;

/* IOCP Reactor 句柄 */
typedef struct RtReactorIocp {
    HANDLE iocp_port;           /* IOCP completion port 句柄 */
    _Atomic(int32_t) pending_count;   /* 未完成 IO 请求数（原子，免热路径锁） */
    _Atomic(uintptr_t) ov_free_head;  /* OVERLAPPED 复用池（tagged CAS 防 ABA） */
    RtIocpTypeEntry type_cache[RT_IOCP_TYPE_CACHE_SIZE];
    RtIocpImmediateQueue immediate;   /* 立即完成事件 FIFO（WOULDBLOCK 合成） */
} RtReactorIocp;

/* ---- 立即完成事件队列 ---- */

static void iocp_immediate_push(RtReactorIocp* r, void* user_data, int32_t fd, int32_t result) {
    if (r->immediate.count >= RT_IOCP_IMMEDIATE_CAP) {
        return; /* 满则丢弃：仅造成一次 read 假挂起，由 Arc 层超时兜底 */
    }
    r->immediate.items[r->immediate.tail].user_data = user_data;
    r->immediate.items[r->immediate.tail].fd = fd;
    r->immediate.items[r->immediate.tail].flags = 0;
    r->immediate.items[r->immediate.tail].result = result;
    r->immediate.tail = (r->immediate.tail + 1) % RT_IOCP_IMMEDIATE_CAP;
    r->immediate.count++;
}

static void iocp_immediate_drain(RtReactorIocp* r, RtIoEvent* events, int32_t max_events, int32_t* n) {
    while (*n < max_events && r->immediate.count > 0) {
        events[*n] = r->immediate.items[r->immediate.head];
        r->immediate.head = (r->immediate.head + 1) % RT_IOCP_IMMEDIATE_CAP;
        r->immediate.count--;
        (*n)++;
    }
}

/* ---- OVERLAPPED 复用池（IO 吞吐预算 · 2026-08-04）----
 * 每 submit 原 calloc + 每完成 free：分配器抖动 + 缺页累积，是 IOCP 后端
 * 吞吐主成本。改为 per-reactor lock-free free-list（treiber stack + tag 防 ABA）：
 * submit 弹池（空则 malloc），poll 完成回池。池结构随 Reactor 销毁（泄漏至
 * 进程退出与既有 rt_reactor 生命周期一致，无迟到访问）。 */
#if UINTPTR_MAX > 0xFFFFFFFFu
#  define IOCP_PTR_MASK 0x0000FFFFFFFFFFFFull
#  define IOCP_TAG_MASK 0xFFFF000000000000ull
#  define IOCP_TAG_ONE  0x0001000000000000ull
#else
#  define IOCP_PTR_MASK 0xFFFFFFFFull
#  define IOCP_TAG_MASK 0xFFFFFFFF00000000ull
#  define IOCP_TAG_ONE  0x100000000ull
#endif

static RtIocpOverlapped* iocp_ov_alloc(RtReactorIocp* r) {
    _Atomic(uintptr_t)* head = &r->ov_free_head;
    uintptr_t old = atomic_load_explicit(head, memory_order_relaxed);
    for (;;) {
        RtIocpOverlapped* ov = (RtIocpOverlapped*)(uintptr_t)(old & IOCP_PTR_MASK);
        if (!ov) break;
        RtIocpOverlapped* next = ov->next;
        uintptr_t upd = ((old + IOCP_TAG_ONE) & IOCP_TAG_MASK)
                      | ((uintptr_t)next & IOCP_PTR_MASK);
        if (atomic_compare_exchange_weak_explicit(head, &old, upd,
                memory_order_acquire, memory_order_relaxed)) {
            /* OVERLAPPED 必须清零后方可复用（Windows 契约） */
            memset(&ov->overlapped, 0, sizeof(ov->overlapped));
            ov->wsa_buf.buf = NULL;
            ov->wsa_buf.len = 0;
            return ov;
        }
    }
    return (RtIocpOverlapped*)calloc(1, sizeof(RtIocpOverlapped));
}

static void iocp_ov_free(RtReactorIocp* r, RtIocpOverlapped* ov) {
    _Atomic(uintptr_t)* head = &r->ov_free_head;
    uintptr_t old = atomic_load_explicit(head, memory_order_relaxed);
    for (;;) {
        ov->next = (RtIocpOverlapped*)(uintptr_t)(old & IOCP_PTR_MASK);
        uintptr_t upd = (old & IOCP_TAG_MASK)
                      | ((uintptr_t)ov & IOCP_PTR_MASK);
        if (atomic_compare_exchange_weak_explicit(head, &old, upd,
                memory_order_release, memory_order_relaxed)) {
            return;
        }
    }
}

/* fd → socket/file 类型缓存查询（命中免 getsockopt syscall） */
static int iocp_fd_is_socket(RtReactorIocp* r, int32_t fd) {
    uint32_t slot = ((uint32_t)(intptr_t)fd) & (RT_IOCP_TYPE_CACHE_SIZE - 1);
    RtIocpTypeEntry* e = &r->type_cache[slot];
    if (e->valid && e->fd == fd) {
        return (int)e->is_socket;
    }
    HANDLE handle = (HANDLE)(intptr_t)fd;
    /* GetFileType 对 Winsock socket 返回 FILE_TYPE_PIPE（非 UNKNOWN），不能作为
     * socket 判据；getsockopt(SO_TYPE) 对 socket 成功、对文件句柄失败
     * （WSAENOTSOCK），是可靠的判据。 */
    int sock_type = 0;
    int optlen = (int)sizeof(sock_type);
    int is_sock = (fd != (int32_t)(intptr_t)INVALID_SOCKET
                   && getsockopt((SOCKET)handle, SOL_SOCKET, SO_TYPE,
                                 (char*)&sock_type, &optlen) == 0);
    e->fd = fd;
    e->is_socket = (uint8_t)is_sock;
    e->valid = 1;
    return is_sock;
}

/* ---- WSAStartup 初始化（首次创建 Reactor 时调用） ---- */
static int g_wsa_initialized = 0;
static void rt_iocp_ensure_wsa(void) {
    if (g_wsa_initialized) return;
    WSADATA wsaData;
    if (WSAStartup(MAKEWORD(2, 2), &wsaData) == 0) {
        g_wsa_initialized = 1;
    }
}

/* ---- impl 接口实现 ---- */

void* rt_reactor_impl_create(uint32_t flags) {
    (void)flags; /* IOCP 不支持 SQPOLL */
    rt_iocp_ensure_wsa();
    RtReactorIocp* r = (RtReactorIocp*)calloc(1, sizeof(RtReactorIocp));
    if (!r) return NULL;
    /* 创建 IOCP，并发数 = 0（按可用 CPU） */
    r->iocp_port = CreateIoCompletionPort(INVALID_HANDLE_VALUE, NULL, 0, 0);
    if (!r->iocp_port) {
        free(r);
        return NULL;
    }
    atomic_init(&r->pending_count, 0);
    atomic_init(&r->ov_free_head, 0);
    return r;
}

void rt_reactor_impl_destroy(void* backend) {
    RtReactorIocp* r = (RtReactorIocp*)backend;
    if (!r) return;
    if (r->iocp_port) {
        CloseHandle(r->iocp_port);
    }
    free(r);
}

int32_t rt_reactor_impl_register(void* backend, int32_t fd, uint32_t events) {
    (void)events;
    RtReactorIocp* r = (RtReactorIocp*)backend;
    if (!r || !r->iocp_port) return -1;
    /* 将 fd（HANDLE/SOCKET）关联到 IOCP。
     * completion key = fd，便于完成时定位。 */
    HANDLE handle = (HANDLE)(intptr_t)fd;
    if (CreateIoCompletionPort(handle, r->iocp_port, (ULONG_PTR)fd, 0) == NULL) {
        /* 已关联过会返回 NULL + ERROR_INVALID_PARAMETER，属正常 */
        if (GetLastError() != ERROR_INVALID_PARAMETER) {
            return -1;
        }
    }
    /* 预热类型缓存：register 后 submit 免 GetFileType syscall（IO 吞吐预算） */
    iocp_fd_is_socket(r, fd);
    return 0;
}

int32_t rt_reactor_impl_modify(void* backend, int32_t fd, uint32_t events) {
    /* IOCP 不需要 modify——事件类型由发起的 IO 操作决定 */
    (void)backend;
    (void)fd;
    (void)events;
    return 0;
}

int32_t rt_reactor_impl_unregister(void* backend, int32_t fd) {
    /* IOCP 无显式注销——关闭 handle 即可 */
    (void)backend;
    (void)fd;
    return 0;
}

int32_t rt_reactor_impl_submit_read(void* backend, int32_t fd, void* buf,
                                     uint32_t len, uint64_t offset, void* user_data) {
    RtReactorIocp* r = (RtReactorIocp*)backend;
    if (!r) return -1;

    RtIocpOverlapped* ov = iocp_ov_alloc(r);
    if (!ov) return -1;
    ov->user_data = user_data;
    ov->op_type = RT_IOCP_OP_READ;
    ov->fd = fd;
    ov->overlapped.Offset = (DWORD)(offset & 0xFFFFFFFF);
    ov->overlapped.OffsetHigh = (DWORD)(offset >> 32);

    HANDLE handle = (HANDLE)(intptr_t)fd;
    DWORD bytes_read = 0;
    BOOL ok;

    /* 判断是 socket 还是文件 handle（类型缓存命中免 GetFileType syscall） */
    if (iocp_fd_is_socket(r, fd)) {
        /* 假设是 socket：用 WSARecv */
        ov->wsa_buf.buf = (char*)buf;
        ov->wsa_buf.len = len;
        DWORD flags = 0;
        ok = (WSARecv((SOCKET)handle, &ov->wsa_buf, 1, &bytes_read, &flags,
                      &ov->overlapped, NULL) == 0);
        if (!ok && WSAGetLastError() != WSA_IO_PENDING) {
            if (WSAGetLastError() == WSAEWOULDBLOCK) {
                /* 非阻塞 socket 无数据就绪：WSARecv 不会投递完成事件，立即以
                 * -1（WANT_READ）完成——与同步 rt_net_recv 的 WOULDBLOCK 语义
                 * 对齐，供 Arc 侧区分「暂时无数据（<0 → 重试）」与「EOF（0）」。
                 * 合成完成入队交 poll 交付（保持 Reactor 与 rt_net.c 解耦，
                 * rt_io_completion_complete 由 EventLoop 对 poll 事件调用）。 */
                iocp_immediate_push(r, ov->user_data, fd, -1);
                iocp_ov_free(r, ov);
                return 0;
            }
            iocp_ov_free(r, ov);
            return -1;
        }
    } else {
        /* 文件 handle：用 ReadFile */
        ok = ReadFile(handle, buf, len, &bytes_read, &ov->overlapped);
        if (!ok && GetLastError() != ERROR_IO_PENDING) {
            iocp_ov_free(r, ov);
            return -1;
        }
    }

    atomic_fetch_add_explicit(&r->pending_count, 1, memory_order_relaxed);
    return 0;
}

int32_t rt_reactor_impl_submit_write(void* backend, int32_t fd, const void* buf,
                                      uint32_t len, uint64_t offset, void* user_data) {
    RtReactorIocp* r = (RtReactorIocp*)backend;
    if (!r) return -1;

    RtIocpOverlapped* ov = iocp_ov_alloc(r);
    if (!ov) {
        return -1;
    }
    ov->user_data = user_data;
    ov->op_type = RT_IOCP_OP_WRITE;
    ov->fd = fd;
    ov->overlapped.Offset = (DWORD)(offset & 0xFFFFFFFF);
    ov->overlapped.OffsetHigh = (DWORD)(offset >> 32);

    HANDLE handle = (HANDLE)(intptr_t)fd;
    DWORD bytes_written = 0;
    BOOL ok;

    if (iocp_fd_is_socket(r, fd)) {
        /* socket: WSASend */
        ov->wsa_buf.buf = (char*)buf;
        ov->wsa_buf.len = len;
        ok = (WSASend((SOCKET)handle, &ov->wsa_buf, 1, &bytes_written, 0,
                      &ov->overlapped, NULL) == 0);
        if (!ok && WSAGetLastError() != WSA_IO_PENDING) {
            iocp_ov_free(r, ov);
            return -1;
        }
    } else {
        /* 文件: WriteFile */
        ok = WriteFile(handle, buf, len, &bytes_written, &ov->overlapped);
        if (!ok && GetLastError() != ERROR_IO_PENDING) {
            iocp_ov_free(r, ov);
            return -1;
        }
    }

    atomic_fetch_add_explicit(&r->pending_count, 1, memory_order_relaxed);
    return 0;
}

int32_t rt_reactor_impl_submit_accept(void* backend, int32_t listen_fd, void* user_data) {
    RtReactorIocp* r = (RtReactorIocp*)backend;
    if (!r) return -1;

    /* AcceptEx 需要 guid 动态加载 */
    static LPFN_ACCEPTEX pfn_accept_ex = NULL;
    if (!pfn_accept_ex) {
        SOCKET s = (SOCKET)(intptr_t)listen_fd;
        DWORD bytes;
        GUID guid = WSAID_ACCEPTEX;
        if (WSAIoctl(s, SIO_GET_EXTENSION_FUNCTION_POINTER, &guid, sizeof(guid),
                     &pfn_accept_ex, sizeof(pfn_accept_ex), &bytes, NULL, NULL) != 0) {
            return -1;
        }
    }

    /* RFC 009 M2 修复：AcceptEx 需要预创建的 accept socket。
     * 根据 listener 的地址族创建同族同类型的 socket。
     * 通过 getsockname 获取 listener 的实际地址族（避免 caller 传入错误信息）。 */
    SOCKET listener = (SOCKET)(intptr_t)listen_fd;
    struct sockaddr_storage local_addr;
    int local_len = sizeof(local_addr);
    int af = AF_INET;  /* 默认 IPv4 */
    if (getsockname(listener, (struct sockaddr*)&local_addr, &local_len) == 0) {
        af = local_addr.ss_family;
    }

    SOCKET accept_sock = socket(af, SOCK_STREAM, IPPROTO_TCP);
    if (accept_sock == INVALID_SOCKET) {
        return -1;
    }

    RtIocpOverlapped* ov = iocp_ov_alloc(r);
    if (!ov) {
        closesocket(accept_sock);
        return -1;
    }
    ov->user_data = user_data;
    ov->op_type = RT_IOCP_OP_ACCEPT;
    ov->fd = listen_fd;
    ov->accept_socket = accept_sock;
    ov->listen_family = af;

    DWORD bytes_received = 0;
    BOOL ok = pfn_accept_ex(listener, accept_sock, ov->accept_buf,
                            0, sizeof(SOCKADDR_IN) + 16, sizeof(SOCKADDR_IN) + 16,
                            &bytes_received, &ov->overlapped);
    if (!ok && WSAGetLastError() != WSA_IO_PENDING) {
        closesocket(accept_sock);
        iocp_ov_free(r, ov);
        return -1;
    }

    atomic_fetch_add_explicit(&r->pending_count, 1, memory_order_relaxed);
    return 0;
}

int32_t rt_reactor_impl_submit_connect(void* backend, int32_t fd,
                                        const void* addr, uint32_t addr_len, void* user_data) {
    RtReactorIocp* r = (RtReactorIocp*)backend;
    if (!r) return -1;

    static LPFN_CONNECTEX pfn_connect_ex = NULL;
    if (!pfn_connect_ex) {
        SOCKET s = (SOCKET)fd;
        DWORD bytes;
        GUID guid = WSAID_CONNECTEX;
        if (WSAIoctl(s, SIO_GET_EXTENSION_FUNCTION_POINTER, &guid, sizeof(guid),
                     &pfn_connect_ex, sizeof(pfn_connect_ex), &bytes, NULL, NULL) != 0) {
            return -1;
        }
    }

    RtIocpOverlapped* ov = iocp_ov_alloc(r);
    if (!ov) return -1;
    ov->user_data = user_data;
    ov->op_type = RT_IOCP_OP_CONNECT;
    ov->fd = fd;

    /* bind 到本地地址（ConnectEx 要求） */
    struct sockaddr_in local;
    memset(&local, 0, sizeof(local));
    local.sin_family = AF_INET;
    local.sin_addr.s_addr = INADDR_ANY;
    local.sin_port = 0;
    bind((SOCKET)fd, (struct sockaddr*)&local, sizeof(local));

    DWORD bytes_sent = 0;
    BOOL ok = pfn_connect_ex((SOCKET)fd, (struct sockaddr*)addr, (int)addr_len,
                             NULL, 0, &bytes_sent, &ov->overlapped);
    if (!ok && WSAGetLastError() != WSA_IO_PENDING) {
        iocp_ov_free(r, ov);
        return -1;
    }

    atomic_fetch_add_explicit(&r->pending_count, 1, memory_order_relaxed);
    return 0;
}

int32_t rt_reactor_impl_flush(void* backend) {
    /* IOCP 无批量提交——每个 IO 发起时立即提交 */
    (void)backend;
    return 0;
}

int32_t rt_reactor_impl_poll(void* backend, RtIoEvent* events, int32_t max_events,
                              int32_t timeout_ms) {
    RtReactorIocp* r = (RtReactorIocp*)backend;
    if (!r || !r->iocp_port) return 0;

    /* 先交付立即完成事件（submit 阶段合成的完成，如非阻塞 socket WOULDBLOCK）。
     * 非阻塞返回——合成完成无 IOCP 等待语义，必须在 GetQueuedCompletionStatusEx
     * 之前交付，否则无事件的 poll 会按 timeout 阻塞。立即事件未计入
     * pending_count，故无需递减。 */
    int32_t n = 0;
    iocp_immediate_drain(r, events, max_events, &n);
    if (n > 0) {
        return n;
    }

    /* GetQueuedCompletionStatusEx 批量获取完成事件 */
    OVERLAPPED_ENTRY entries[64];
    int32_t to_take = max_events < 64 ? max_events : 64;
    ULONG num_removed = 0;

    DWORD timeout = (timeout_ms < 0) ? INFINITE : (DWORD)timeout_ms;
    BOOL ok = GetQueuedCompletionStatusEx(r->iocp_port, entries, (ULONG)to_take,
                                          &num_removed, timeout, FALSE);
    if (!ok) {
        /* WAIT_TIMEOUT 是正常的（无事件） */
        if (GetLastError() == WAIT_TIMEOUT) return 0;
        return -1;
    }

    for (ULONG i = 0; i < num_removed && n < max_events; i++) {
        RtIocpOverlapped* ov = (RtIocpOverlapped*)entries[i].lpOverlapped;
        if (!ov) continue;

        events[n].user_data = ov->user_data;
        events[n].fd = ov->fd;
        events[n].flags = 0;

        /* 检查 IO 结果 */
        DWORD bytes_transferred = entries[i].dwNumberOfBytesTransferred;
        if (ov->op_type == RT_IOCP_OP_CONNECT) {
            /* ConnectEx 完成时 bytes_transferred=0；错误码在 overlapped.Internal */
            DWORD ov_err = (DWORD)ov->overlapped.Internal;
            if (ov_err != 0) {
                events[n].result = -1;
            } else {
                events[n].result = 0;
            }
        } else if (ov->op_type == RT_IOCP_OP_ACCEPT) {
            /* RFC 009 M2: accept 完成时，result = 新 accept socket 的 fd。
             * AcceptEx 完成后 accept_socket 已绑定到客户端连接，
             * 通过 SO_UPDATE_ACCEPT_CONTEXT 使其可正常收发数据。 */
            SOCKET accept_sock = ov->accept_socket;
            /* 更新 accept socket 的上下文（让 getsockname/getpeername 可用） */
            SOCKET listener = (SOCKET)(intptr_t)ov->fd;
            setsockopt(accept_sock, SOL_SOCKET, SO_UPDATE_ACCEPT_CONTEXT,
                       (const char*)&listener, sizeof(listener));
            events[n].result = (int32_t)(intptr_t)accept_sock;
        } else if (bytes_transferred == 0) {
            /* 0 字节读取通常表示对端关闭 */
            events[n].result = 0;  /* 返回 0 让上层处理 EOF */
        } else {
            events[n].result = (int32_t)bytes_transferred;
        }

        iocp_ov_free(r, ov);
        n++;
    }

    if (n > 0) {
        atomic_fetch_sub_explicit(&r->pending_count, n, memory_order_relaxed);
    }

    return n;
}

/* RFC 009 M6: 跨线程唤醒 —— PostQueuedCompletionStatus 注入哨兵事件。
 * 哨兵 entry 的 lpOverlapped=NULL（poll 循环 `if (!ov) continue` 跳过，不交付
 * 任何完成事件），仅使阻塞的 GetQueuedCompletionStatusEx 立即返回。线程安全。
 * 用于多线程 executor：worker 线程在根任务完成时唤醒 EventLoop 驱动线程。 */
void rt_reactor_impl_wake(void* backend) {
    RtReactorIocp* r = (RtReactorIocp*)backend;
    if (!r || !r->iocp_port) return;
    PostQueuedCompletionStatus(r->iocp_port, 0, 0, NULL);
}

int32_t rt_reactor_impl_register_buffers(void* backend, const void** buffers,
                                          const uint32_t* lengths, int32_t n) {
    /* IOCP 无原生缓冲池注册——记录 buffer 供后续使用（模拟） */
    (void)backend;
    (void)buffers;
    (void)lengths;
    (void)n;
    return 0;  /* 静默成功：降级为普通 buffer */
}

const char* rt_reactor_impl_backend_name(void) {
    return "iocp";
}

void rt_reactor_impl_set_link_flag(void* backend, int32_t enable) {
    (void)backend; (void)enable; /* IOCP 不支持链式操作 */
}

int32_t rt_reactor_impl_submit_timeout(void* backend, uint64_t timeout_ns, void* user_data) {
    (void)backend; (void)timeout_ns; (void)user_data;
    return -1; /* IOCP 不支持 timeout 提交 */
}

#endif /* _WIN32 */
