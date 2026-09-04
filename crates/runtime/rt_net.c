// Network ABI (RFC 025 M4: Arc.Net runtime).
//
// Implements the cross-platform socket + DNS primitives backing the Arc.Net
// std facade. All public entry points operate on an opaque RtSocket* handle;
// the handle is a malloc'd struct that wraps the OS-level socket fd/SOCKET.
//
// Windows uses Winsock2 (ws2_32.lib), initialised lazily on first socket
// creation. Unix targets use POSIX sockets (<sys/socket.h>, <netdb.h>).
//
// Error model: all operations return 0/NULL on failure, positive values on
// success. The caller (ARC codegen) translates results into Arc types.
// All functions are thread-safe 鈥?no mutable global state beyond the
// one-time Winsock initialisation (guarded by a static flag + critical
// section equivalent).
//
// Memory model: rt_socket_receive and rt_dns_resolve return freshly malloc'd
// NUL-terminated strings; the caller (ARC runtime) owns the returned memory.
// rt_socket_accept returns a newly allocated RtSocket* handle.

#include "rt_abi.h"

#include <stdlib.h>
#include <string.h>
#include <stdio.h>

#if defined(_WIN32)
#  define WIN32_LEAN_AND_MEAN
#  include <windows.h>
#  include <winsock2.h>
#  include <ws2tcpip.h>
#  pragma comment(lib, "ws2_32.lib")
#else
#  include <sys/socket.h>
#  include <sys/ioctl.h>
#  include <netinet/in.h>
#  include <netinet/tcp.h>
#  include <arpa/inet.h>
#  include <netdb.h>
#  include <unistd.h>
#  include <fcntl.h>
#  include <errno.h>
#  define INVALID_SOCKET (-1)
#  define SOCKET_ERROR   (-1)
#endif

/* ---- RtSocket handle definition ---------------------------------------- */

/* The opaque handle returned by rt_socket_create. ARC codegen stores this
 * pointer directly in the Socket object's payload. */
typedef struct RtSocket {
#if defined(_WIN32)
    SOCKET fd;
#else
    int    fd;
#endif
    int    closed;       /* 0=open, 1=gracefully closed by rt_socket_close */
} RtSocket;

/* Platform-specific raw socket descriptor type (avoids UB from casting
 * stack scalars to RtSocket* — close_raw_socket takes the raw descriptor). */
#if defined(_WIN32)
typedef SOCKET rt_sock_fd_t;
#define RT_INVALID_SOCKET INVALID_SOCKET
#else
typedef int    rt_sock_fd_t;
#define RT_INVALID_SOCKET (-1)
#endif

/* ---- Platform helpers -------------------------------------------------- */

#if defined(_WIN32)

/* Lazy one-time Winsock initialisation. Thread-safe via InitOnce. */
static INIT_ONCE g_winsock_init_once = INIT_ONCE_STATIC_INIT;
static BOOL      g_winsock_ok         = FALSE;

static BOOL CALLBACK winsock_init_cb(PINIT_ONCE _once, PVOID _param, PVOID *_ctx) {
    WSADATA wsa;
    g_winsock_ok = (WSAStartup(MAKEWORD(2, 2), &wsa) == 0);
    return TRUE;
}

static int ensure_winsock(void) {
    InitOnceExecuteOnce(&g_winsock_init_once, winsock_init_cb, NULL, NULL);
    return g_winsock_ok ? 0 : -1;
}

static int last_net_error(void) { return WSAGetLastError(); }
#define NET_EWOULDBLOCK WSAEWOULDBLOCK

static void close_socket_fd(RtSocket* s) {
    if (!s) return;
    if (s->fd != INVALID_SOCKET) {
        closesocket(s->fd);
        s->fd = INVALID_SOCKET;
    }
}

static void close_raw_socket(rt_sock_fd_t fd) {
    if (fd != INVALID_SOCKET) {
        closesocket(fd);
    }
}

#else /* Unix */

static int ensure_winsock(void) { return 0; }  /* no-op */

static int last_net_error(void) { return errno; }
#define NET_EWOULDBLOCK EWOULDBLOCK

static void close_socket_fd(RtSocket* s) {
    if (!s) return;
    if (s->fd >= 0) {
        close(s->fd);
        s->fd = -1;
    }
}

static void close_raw_socket(rt_sock_fd_t fd) {
    if (fd >= 0) {
        close(fd);
    }
}

#endif

/* ---- Socket lifecycle -------------------------------------------------- */

/* Address family constants — must match Arc.Net.AddressFamily enum
 * declaration order: InterNetwork=0, InterNetworkV6=1. */
#define RT_AF_INET   0
#define RT_AF_INET6  1

/* Socket type constants — must match Arc.Net.SocketType enum
 * declaration order: Stream=0, Dgram=1. */
#define RT_SOCK_STREAM 0
#define RT_SOCK_DGRAM  1

/* Protocol constants — must match Arc.Net.ProtocolType enum
 * declaration order: Tcp=0, Udp=1. */
#define RT_PROTO_TCP 0
#define RT_PROTO_UDP 1

static int to_native_af(int32_t af) {
    switch (af) {
        case RT_AF_INET:  return AF_INET;
        case RT_AF_INET6: return AF_INET6;
        default:          return AF_INET;
    }
}

static int to_native_socktype(int32_t st) {
    switch (st) {
        case RT_SOCK_STREAM: return SOCK_STREAM;
        case RT_SOCK_DGRAM:  return SOCK_DGRAM;
        default:             return SOCK_STREAM;
    }
}

static int to_native_proto(int32_t pt) {
    switch (pt) {
        case RT_PROTO_TCP: return IPPROTO_TCP;
        case RT_PROTO_UDP: return IPPROTO_UDP;
        default:           return IPPROTO_TCP;
    }
}

void* rt_socket_create(int32_t addressFamily, int32_t socketType,
                     int32_t protocolType) {
    if (ensure_winsock() != 0) return NULL;

    int af    = to_native_af(addressFamily);
    int type  = to_native_socktype(socketType);
    int proto = to_native_proto(protocolType);

#if defined(_WIN32)
    SOCKET fd = socket(af, type, proto);
#else
    int fd = socket(af, type, proto);
#endif
    if (fd == INVALID_SOCKET) return NULL;

    /* RFC 050 M-a：opaque 统一头试点（对象自描述身份，ARC 误计数物理无害）。 */
    RtSocket* s = (RtSocket*)rt_obj_alloc_opaque(sizeof(RtSocket));
    if (!s) {
        close_raw_socket(fd);
        return NULL;
    }
    s->fd     = fd;
    s->closed = 0;
    return (void*)s;
}

void rt_socket_close(void* handle) {
    if (!handle) return;
    RtSocket* s = (RtSocket*)handle;
    if (s->closed) return;
    s->closed = 1;
    close_socket_fd(s);
    rt_obj_free(s);
}

/* ---- Connection management --------------------------------------------- */

int32_t rt_socket_connect(void* handle, const char* host, int32_t port) {
    if (!handle || !host) return 0;
    RtSocket* s = (RtSocket*)handle;
    if (s->closed || s->fd == INVALID_SOCKET) return 0;

    /* Resolve the hostname to an IP address first. */
    struct addrinfo hints, *result = NULL;
    memset(&hints, 0, sizeof(hints));
    hints.ai_family   = AF_UNSPEC;
    hints.ai_socktype = SOCK_STREAM;

    char port_str[16];
    snprintf(port_str, sizeof(port_str), "%d", (int)port);

    if (getaddrinfo((const char*)host, port_str, &hints, &result) != 0) {
        return 0;
    }

    /* Try each address until one connects. */
    int32_t ok = 0;
    for (struct addrinfo* rp = result; rp != NULL; rp = rp->ai_next) {
#if defined(_WIN32)
        if (connect(s->fd, rp->ai_addr, (int)rp->ai_addrlen) != SOCKET_ERROR) {
#else
        if (connect(s->fd, rp->ai_addr, rp->ai_addrlen) == 0) {
#endif
            ok = 1;
            break;
        }
    }
    freeaddrinfo(result);

    /* Enable TCP_NODELAY for responsive sends. */
    if (ok) {
        int nodelay = 1;
        setsockopt(s->fd, IPPROTO_TCP, TCP_NODELAY,
                   (const char*)&nodelay, sizeof(nodelay));
    }
    return ok;
}

int32_t rt_socket_bind(void* handle, int32_t port) {
    if (!handle) return 0;
    RtSocket* s = (RtSocket*)handle;
    if (s->closed || s->fd == INVALID_SOCKET) return 0;

    /* SO_REUSEADDR：TcpListener.Start 环回 / 短测重绑更稳；非协议扩张。 */
#if defined(_WIN32)
    {
        BOOL reuse = TRUE;
        setsockopt(s->fd, SOL_SOCKET, SO_REUSEADDR, (const char*)&reuse, sizeof(reuse));
    }
#else
    {
        int reuse = 1;
        setsockopt(s->fd, SOL_SOCKET, SO_REUSEADDR, &reuse, sizeof(reuse));
    }
#endif

    struct sockaddr_in addr;
    memset(&addr, 0, sizeof(addr));
    addr.sin_family      = AF_INET;
    addr.sin_port        = htons((uint16_t)port);
    addr.sin_addr.s_addr = INADDR_ANY;

#if defined(_WIN32)
    return (bind(s->fd, (struct sockaddr*)&addr, sizeof(addr)) != SOCKET_ERROR) ? 1 : 0;
#else
    return (bind(s->fd, (struct sockaddr*)&addr, sizeof(addr)) == 0) ? 1 : 0;
#endif
}

int32_t rt_socket_listen(void* handle, int32_t backlog) {
    if (!handle) return 0;
    RtSocket* s = (RtSocket*)handle;
    if (s->closed || s->fd == INVALID_SOCKET) return 0;

#if defined(_WIN32)
    return (listen(s->fd, (int)backlog) != SOCKET_ERROR) ? 1 : 0;
#else
    return (listen(s->fd, (int)backlog) == 0) ? 1 : 0;
#endif
}

void* rt_socket_accept(void* handle) {
    if (!handle) return NULL;
    RtSocket* s = (RtSocket*)handle;
    if (s->closed || s->fd == INVALID_SOCKET) return NULL;

    struct sockaddr_in client_addr;
    socklen_t addr_len = sizeof(client_addr);
    memset(&client_addr, 0, sizeof(client_addr));

#if defined(_WIN32)
    SOCKET client_fd = accept(s->fd, (struct sockaddr*)&client_addr, &addr_len);
#else
    int client_fd = accept(s->fd, (struct sockaddr*)&client_addr, &addr_len);
#endif
    if (client_fd == INVALID_SOCKET) return NULL;

    /* Enable TCP_NODELAY on accepted sockets too. */
    int nodelay = 1;
    setsockopt(client_fd, IPPROTO_TCP, TCP_NODELAY,
               (const char*)&nodelay, sizeof(nodelay));

    /* RFC 050 M-a：opaque 统一头试点（accept 侧同 create）。 */
    RtSocket* cs = (RtSocket*)rt_obj_alloc_opaque(sizeof(RtSocket));
    if (!cs) {
        close_raw_socket(client_fd);
        return NULL;
    }
    cs->fd     = client_fd;
    cs->closed = 0;
    return (void*)cs;
}

/* ---- Data transfer ----------------------------------------------------- */

int32_t rt_socket_send(void* handle, const void* data, int32_t length) {
    if (!handle || !data || length <= 0) return 0;
    RtSocket* s = (RtSocket*)handle;
    if (s->closed || s->fd == INVALID_SOCKET) return 0;

#if defined(_WIN32)
    int sent = send(s->fd, (const char*)data, length, 0);
#else
    ssize_t sent = send(s->fd, (const char*)data, length, MSG_NOSIGNAL);
#endif
    return (sent > 0) ? (int32_t)sent : 0;
}

/* 非阻塞 socket 上的阻塞读等待：select 等待可读，预算取 SO_RCVTIMEO。
 * 返回 1=可读，0=超时/错误。异步路径（Reactor 收/发）会把 socket 切非阻塞，
 * 同步 rt_socket_receive 随后 recv 立即得 WSAEWOULDBLOCK——这里模拟阻塞语义，
 * 避免把「暂无数据」误判为 EOF。 */
static int rt_socket_wait_readable(RtSocket* s) {
    if (!s || s->closed || s->fd == INVALID_SOCKET) return 0;
    fd_set fds;
    FD_ZERO(&fds);
#if defined(_WIN32)
    FD_SET(s->fd, &fds);
#else
    if (s->fd >= FD_SETSIZE) return 0;
    FD_SET(s->fd, &fds);
#endif

    struct timeval tv;
    struct timeval* ptv = NULL;
#if defined(_WIN32)
    DWORD ms = 0;
    int optlen = (int)sizeof(ms);
    if (getsockopt(s->fd, SOL_SOCKET, SO_RCVTIMEO, (char*)&ms, &optlen) == 0 && ms > 0) {
        tv.tv_sec  = ms / 1000;
        tv.tv_usec = (ms % 1000) * 1000;
        ptv = &tv;
    }
#else
    struct timeval rcv;
    socklen_t optlen = sizeof(rcv);
    if (getsockopt(s->fd, SOL_SOCKET, SO_RCVTIMEO, &rcv, &optlen) == 0 &&
        (rcv.tv_sec > 0 || rcv.tv_usec > 0)) {
        tv = rcv;
        ptv = &tv;
    }
#endif

    int ret = select((int)(s->fd + 1), &fds, NULL, NULL, ptv);
    return (ret > 0) ? 1 : 0;
}

void* rt_socket_receive(void* handle, int32_t bufferSize) {
    if (!handle || bufferSize <= 0) return NULL;
    RtSocket* s = (RtSocket*)handle;
    if (s->closed || s->fd == INVALID_SOCKET) return NULL;

    char* buf = (char*)malloc((size_t)bufferSize + 1);
    if (!buf) return NULL;

    /* 重试预算：非阻塞 socket 上 WSAEWOULDBLOCK 不代表 EOF，select 等待后重试。 */
    for (int attempt = 0; attempt < 4096; attempt++) {
#if defined(_WIN32)
        int n = recv(s->fd, buf, (int)bufferSize, 0);
        if (n > 0) {
            buf[n] = '\0';
            return (void*)buf;
        }
        if (n == 0) {
            free(buf);
            return NULL; /* 对端关闭（真 EOF） */
        }
        /* n == SOCKET_ERROR */
        if (WSAGetLastError() != WSAEWOULDBLOCK) {
            free(buf);
            return NULL;
        }
#else
        ssize_t n = recv(s->fd, buf, (size_t)bufferSize, 0);
        if (n > 0) {
            buf[n] = '\0';
            return (void*)buf;
        }
        if (n == 0) {
            free(buf);
            return NULL; /* 对端关闭（真 EOF） */
        }
        if (errno != EAGAIN && errno != EWOULDBLOCK) {
            free(buf);
            return NULL;
        }
#endif
        /* 非阻塞无数据就绪：等待可读（预算 SO_RCVTIMEO）后重试 recv。 */
        if (!rt_socket_wait_readable(s)) {
            free(buf);
            return NULL; /* 超时 / 连接已关 */
        }
    }
    free(buf);
    return NULL;
}

/* RFC 025 §1.2.g / RFC 025 M0（2026-08-05 数据报级 byte[] 升级 · 逐项独立立宪）：
 * 数据报级 sendto——向远端 host:port 发一个数据报，不 connect（sendto 语义）。
 * data 为调用方 byte[] 载荷指针（元素 0），length 为显式长度（内部 0x00 完整发送）。
 * 返回实际发送字节数（≤ length；失败 0）。 */
int32_t rt_socket_sendto_bytes(void* handle, const void* data, int32_t length,
                               const char* host, int32_t port) {
    if (!handle || !data || length <= 0 || !host) return 0;
    RtSocket* s = (RtSocket*)handle;
    if (s->closed || s->fd == INVALID_SOCKET) return 0;

    struct addrinfo hints;
    struct addrinfo* res = NULL;
    memset(&hints, 0, sizeof(hints));
    hints.ai_family   = AF_UNSPEC;
    hints.ai_socktype = SOCK_DGRAM;  /* UDP 数据报目标解析 */

    char port_str[16];
    snprintf(port_str, sizeof(port_str), "%d", (int)port);

    if (getaddrinfo(host, port_str, &hints, &res) != 0 || !res) {
        if (res) freeaddrinfo(res);
        return 0;
    }

    int32_t sent = 0;
#if defined(_WIN32)
    int n = sendto(s->fd, (const char*)data, length, 0, res->ai_addr, (int)res->ai_addrlen);
    if (n != SOCKET_ERROR) sent = n;
#else
    ssize_t n = sendto(s->fd, (const char*)data, length, MSG_NOSIGNAL,
                       res->ai_addr, res->ai_addrlen);
    if (n > 0) sent = (int32_t)n;
#endif
    freeaddrinfo(res);
    return sent;
}

/* 数据报级 recvfrom——收一个数据报到调用方 buffer（忽略源地址）。
 * 返回实际收到的数据报字节数（≤ bufferSize；失败/超时 0）。
 * 数据报大于 bufferSize 时按 UDP 语义截断（仅保留前 bufferSize 字节）。 */
int32_t rt_socket_recvfrom_bytes(void* handle, void* buffer, int32_t bufferSize) {
    if (!handle || !buffer || bufferSize <= 0) return 0;
    RtSocket* s = (RtSocket*)handle;
    if (s->closed || s->fd == INVALID_SOCKET) return 0;

#if defined(_WIN32)
    int n = recvfrom(s->fd, (char*)buffer, bufferSize, 0, NULL, NULL);
    return (n > 0) ? (int32_t)n : 0;
#else
    ssize_t n = recvfrom(s->fd, (char*)buffer, (size_t)bufferSize, 0, NULL, NULL);
    return (n > 0) ? (int32_t)n : 0;
#endif
}

/* ---- Socket options ---------------------------------------------------- */

int32_t rt_socket_available(void* handle) {
    if (!handle) return 0;
    RtSocket* s = (RtSocket*)handle;
    if (s->closed || s->fd == INVALID_SOCKET) return 0;

#if defined(_WIN32)
    u_long avail = 0;
    if (ioctlsocket(s->fd, FIONREAD, &avail) != 0) return 0;
    return (int32_t)avail;
#else
    int avail = 0;
    if (ioctl(s->fd, FIONREAD, &avail) != 0) return 0;
    return (int32_t)avail;
#endif
}

void rt_socket_set_recv_timeout(void* handle, int32_t ms) {
    if (!handle) return;
    RtSocket* s = (RtSocket*)handle;
    if (s->closed || s->fd == INVALID_SOCKET) return;

#if defined(_WIN32)
    DWORD tv = (DWORD)ms;
    setsockopt(s->fd, SOL_SOCKET, SO_RCVTIMEO,
               (const char*)&tv, sizeof(tv));
#else
    struct timeval tv;
    tv.tv_sec  = ms / 1000;
    tv.tv_usec = (ms % 1000) * 1000;
    setsockopt(s->fd, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof(tv));
#endif
}

void rt_socket_set_send_timeout(void* handle, int32_t ms) {
    if (!handle) return;
    RtSocket* s = (RtSocket*)handle;
    if (s->closed || s->fd == INVALID_SOCKET) return;

#if defined(_WIN32)
    DWORD tv = (DWORD)ms;
    setsockopt(s->fd, SOL_SOCKET, SO_SNDTIMEO,
               (const char*)&tv, sizeof(tv));
#else
    struct timeval tv;
    tv.tv_sec  = ms / 1000;
    tv.tv_usec = (ms % 1000) * 1000;
    setsockopt(s->fd, SOL_SOCKET, SO_SNDTIMEO, &tv, sizeof(tv));
#endif
}

int32_t rt_socket_connected(void* handle) {
    if (!handle) return 0;
    RtSocket* s = (RtSocket*)handle;
    if (s->closed || s->fd == INVALID_SOCKET) return 0;

    int err = 0;
    socklen_t len = sizeof(err);
    if (getsockopt(s->fd, SOL_SOCKET, SO_ERROR, (char*)&err, &len) != 0) {
        return 0;
    }
    return (err == 0) ? 1 : 0;
}

void rt_socket_shutdown(void* handle, int32_t how) {
    if (!handle) return;
    RtSocket* s = (RtSocket*)handle;
    if (s->closed || s->fd == INVALID_SOCKET) return;

#if defined(_WIN32)
    int sd_how = (how == 0) ? SD_RECEIVE : (how == 1) ? SD_SEND : SD_BOTH;
    shutdown(s->fd, sd_how);
#else
    int sh_how = (how == 0) ? SHUT_RD : (how == 1) ? SHUT_WR : SHUT_RDWR;
    shutdown(s->fd, sh_how);
#endif
}

int32_t rt_socket_poll(void* handle, int32_t microSeconds, int32_t mode) {
    if (!handle) return 0;
    RtSocket* s = (RtSocket*)handle;
    if (s->closed || s->fd == INVALID_SOCKET) return 0;

    fd_set fds;
    FD_ZERO(&fds);
#if defined(_WIN32)
    FD_SET(s->fd, &fds);
#else
    if (s->fd >= FD_SETSIZE) return 0;
    FD_SET(s->fd, &fds);
#endif

    struct timeval tv;
    struct timeval* ptv = NULL;
    if (microSeconds >= 0) {
        tv.tv_sec  = microSeconds / 1000000;
        tv.tv_usec = microSeconds % 1000000;
        ptv = &tv;
    }

    int ret;
    switch (mode) {
        case 0: /* Read */
            ret = select((int)(s->fd + 1), &fds, NULL, NULL, ptv);
            break;
        case 1: /* Write */
            ret = select((int)(s->fd + 1), NULL, &fds, NULL, ptv);
            break;
        case 2: /* Error */
            ret = select((int)(s->fd + 1), NULL, NULL, &fds, ptv);
            break;
        default:
            return 0;
    }

    return (ret > 0) ? 1 : 0;
}

void rt_socket_set_no_delay(void* handle, int32_t noDelay) {
    if (!handle) return;
    RtSocket* s = (RtSocket*)handle;
    if (s->closed || s->fd == INVALID_SOCKET) return;

    int val = noDelay ? 1 : 0;
    setsockopt(s->fd, IPPROTO_TCP, TCP_NODELAY, (const char*)&val, sizeof(val));
}

void rt_socket_set_send_buf_size(void* handle, int32_t size) {
    if (!handle || size <= 0) return;
    RtSocket* s = (RtSocket*)handle;
    if (s->closed || s->fd == INVALID_SOCKET) return;

    setsockopt(s->fd, SOL_SOCKET, SO_SNDBUF, (const char*)&size, sizeof(size));
}

void rt_socket_set_recv_buf_size(void* handle, int32_t size) {
    if (!handle || size <= 0) return;
    RtSocket* s = (RtSocket*)handle;
    if (s->closed || s->fd == INVALID_SOCKET) return;

    setsockopt(s->fd, SOL_SOCKET, SO_RCVBUF, (const char*)&size, sizeof(size));
}

/* ---- RFC 009 M2: 异步网络 IO facade -------------------------------------- */
/*
 * 异步入口：创建 Pending Task + RtIoCompletion 上下文，提交到当前 EventLoop
 * 绑定的 Reactor。Reactor 完成后 EventLoop tick 调用 rt_io_completion_complete
 * 把结果写回 Task 并触发 waker。
 *
 * RtIoCompletion 上下文记录 op_type + Task + buffer，作为 submit_* 的 user_data。
 * 完成时由 rt_io_completion_complete 释放（buffer 所有权可能转移到 Task 结果）。
 *
 * RtIoOpType / RtIoCompletion 定义在 rt_abi.h（网络/文件共享），此处复用。
 *
 * 操作类型：
 *   0 = connect：result 0=成功，<0=错误（写 int_result 1/0）
 *   1 = accept  ：result = 新 fd（包装为 RtSocket* 写 ptr_result）
 *   2 = read    ：result = 字节数（写 int_result + buf 转 string 写 ptr_result）
 *   3 = write   ：result = 字节数（写 int_result）
 */

/* 设置 socket 为非阻塞模式（内部辅助，不导出）。 */
static void rt_socket_set_nonblocking_internal(RtSocket* s) {
    if (!s || s->closed) return;
#if defined(_WIN32)
    u_long mode = 1;
    ioctlsocket(s->fd, FIONBIO, &mode);
#else
    int flags = fcntl(s->fd, F_GETFL, 0);
    if (flags >= 0) {
        fcntl(s->fd, F_SETFL, flags | O_NONBLOCK);
    }
#endif
}

/* RtSocket.fd 转 int32_t（Reactor API 使用 int32_t fd）。 */
static int32_t rt_socket_fd_for_reactor(RtSocket* s) {
    if (!s || s->closed) return -1;
#if defined(_WIN32)
    return (int32_t)(intptr_t)s->fd;
#else
    return s->fd;
#endif
}

/* 解析 host:port 为 sockaddr（用于 connect）。返回 addr_len，失败返回 0。 */
static uint32_t rt_resolve_addr(const char* host, int32_t port,
                                  struct sockaddr_storage* out) {
    if (!host || !out) return 0;
    memset(out, 0, sizeof(*out));

    /* 先尝试数值 IP */
    struct sockaddr_in* addr4 = (struct sockaddr_in*)out;
    addr4->sin_family = AF_INET;
    addr4->sin_port = htons((u_short)port);
    if (inet_pton(AF_INET, host, &addr4->sin_addr) == 1) {
        return sizeof(struct sockaddr_in);
    }

    /* IPv6 */
    struct sockaddr_in6* addr6 = (struct sockaddr_in6*)out;
    addr6->sin6_family = AF_INET6;
    addr6->sin6_port = htons((u_short)port);
    if (inet_pton(AF_INET6, host, &addr6->sin6_addr) == 1) {
        return sizeof(struct sockaddr_in6);
    }

    /* DNS 解析：优先 IPv4（Arc `TcpClient` 默认 AF_INET socket；ConnectEx 要求
     * 目标地址族与 socket 一致）。同步 rt_socket_connect 会遍历所有结果重试，
     * 异步 ConnectEx 只取一个——故这里挑选首个 IPv4，无 IPv4 时回退 IPv6。 */
    struct addrinfo hints, *res = NULL;
    memset(&hints, 0, sizeof(hints));
    hints.ai_family = AF_UNSPEC;
    hints.ai_socktype = SOCK_STREAM;
    char port_str[16];
    snprintf(port_str, sizeof(port_str), "%d", port);
    if (getaddrinfo(host, port_str, &hints, &res) != 0 || !res) {
        return 0;
    }
    struct addrinfo* pick = NULL;
    for (struct addrinfo* rp = res; rp != NULL; rp = rp->ai_next) {
        if (rp->ai_family == AF_INET) {
            pick = rp;
            break;
        }
    }
    if (pick == NULL) {
        pick = res;
    }
    uint32_t len = (uint32_t)pick->ai_addrlen;
    memcpy(out, pick->ai_addr, len < sizeof(*out) ? len : sizeof(*out));
    freeaddrinfo(res);
    return len;
}

void* rt_socket_connect_async(void* handle, const char* host, int32_t port) {
    RtSocket* s = (RtSocket*)handle;
    if (!s || s->closed || !host) return NULL;

    /* 获取当前 EventLoop 的 Reactor */
    void* loop = rt_event_loop_current();
    void* reactor = loop ? rt_event_loop_get_reactor(loop) : NULL;
    if (!reactor) {
        return NULL;
    }

    /* 设置非阻塞 */
    rt_socket_set_nonblocking_internal(s);

    /* 解析地址 */
    struct sockaddr_storage addr;
    uint32_t addr_len = rt_resolve_addr(host, port, &addr);
    if (addr_len == 0) {
        return NULL;
    }

    /* 创建 Pending Task + completion 上下文 */
    RtTask* task = rt_task_alloc();
    if (!task) return NULL;
    task->status = RT_TASK_PENDING;

    RtIoCompletion* compl = (RtIoCompletion*)calloc(1, sizeof(RtIoCompletion));
    if (!compl) {
        rt_task_release(task);
        return NULL;
    }
    compl->task = task;
    compl->op_type = RT_IO_OP_CONNECT;

    /* 提交 connect 到 Reactor。
     * 注意：connect 需要先 bind 到本地地址（ConnectEx 要求）。
     * IOCP：fd 必须先通过 rt_reactor_register 关联到 IOCP port。 */
    int32_t fd = rt_socket_fd_for_reactor(s);
    rt_reactor_register(reactor, fd, 0);
    int32_t rc = rt_reactor_submit_connect(reactor, fd, &addr, addr_len, compl);
    if (rc != 0) {
        free(compl);
        rt_task_release(task);
        return NULL;
    }
    return task;
}

void* rt_socket_accept_async(void* handle) {
    RtSocket* s = (RtSocket*)handle;
    if (!s || s->closed) {
        if (getenv("ARC_DEBUG_NET")) {
            fprintf(stderr, "[net-dbg] accept_async NULL: closed-branch handle=%p closed=%d\n",
                    (void*)s, s ? (int)s->closed : -1);
        }
        return NULL;
    }

    void* loop = rt_event_loop_current();
    void* reactor = loop ? rt_event_loop_get_reactor(loop) : NULL;
    if (!reactor) {
        if (getenv("ARC_DEBUG_NET")) {
            fprintf(stderr, "[net-dbg] accept_async NULL: reactor-branch loop=%p reactor=%p\n",
                    loop, reactor);
        }
        return NULL;
    }

    rt_socket_set_nonblocking_internal(s);

    RtTask* task = rt_task_alloc();
    if (!task) return NULL;
    task->status = RT_TASK_PENDING;

    RtIoCompletion* compl = (RtIoCompletion*)calloc(1, sizeof(RtIoCompletion));
    if (!compl) {
        rt_task_release(task);
        return NULL;
    }
    compl->task = task;
    compl->op_type = RT_IO_OP_ACCEPT;

    /* IOCP：listener fd 必须先关联到 IOCP port。 */
    int32_t fd = rt_socket_fd_for_reactor(s);
    rt_reactor_register(reactor, fd, 0);
    int32_t rc = rt_reactor_submit_accept(reactor, fd, compl);
    if (rc != 0) {
        if (getenv("ARC_DEBUG_NET")) {
            fprintf(stderr, "[net-dbg] accept_async NULL: submit-branch rc=%d fd=%d\n", rc, fd);
        }
        free(compl);
        rt_task_release(task);
        return NULL;
    }
    return task;
}

void* rt_socket_send_async(void* handle, const void* data, int32_t length) {
    RtSocket* s = (RtSocket*)handle;
    if (!s || s->closed || !data || length <= 0) {
        return NULL;
    }

    void* loop = rt_event_loop_current();
    void* reactor = loop ? rt_event_loop_get_reactor(loop) : NULL;
    if (!reactor) {
        return NULL;
    }

    rt_socket_set_nonblocking_internal(s);

    RtTask* task = rt_task_alloc();
    if (!task) return NULL;
    task->status = RT_TASK_PENDING;

    RtIoCompletion* compl = (RtIoCompletion*)calloc(1, sizeof(RtIoCompletion));
    if (!compl) {
        rt_task_release(task);
        return NULL;
    }
    compl->task = task;
    compl->op_type = RT_IO_OP_WRITE;

    /* IOCP：fd 必须先关联到 IOCP port。 */
    int32_t fd = rt_socket_fd_for_reactor(s);
    rt_reactor_register(reactor, fd, 0);
    int32_t rc = rt_reactor_submit_write(reactor, fd, data, (uint32_t)length, 0, compl);
    if (rc != 0) {
        free(compl);
        rt_task_release(task);
        return NULL;
    }
    return task;
}

void* rt_socket_receive_async(void* handle, int32_t bufferSize) {
    RtSocket* s = (RtSocket*)handle;
    if (!s || s->closed || bufferSize <= 0) return NULL;

    void* loop = rt_event_loop_current();
    void* reactor = loop ? rt_event_loop_get_reactor(loop) : NULL;
    if (!reactor) return NULL;

    rt_socket_set_nonblocking_internal(s);

    /* 分配接收 buffer（+1 用于 NUL 终止，便于后续转为 string） */
    char* buf = (char*)calloc(1, (size_t)bufferSize + 1);
    if (!buf) return NULL;

    RtTask* task = rt_task_alloc();
    if (!task) {
        free(buf);
        return NULL;
    }
    task->status = RT_TASK_PENDING;

    RtIoCompletion* compl = (RtIoCompletion*)calloc(1, sizeof(RtIoCompletion));
    if (!compl) {
        rt_task_release(task);
        free(buf);
        return NULL;
    }
    compl->task = task;
    compl->op_type = RT_IO_OP_READ;
    compl->buf = buf;
    compl->buf_size = bufferSize;

    /* IOCP：fd 必须先关联到 IOCP port。 */
    int32_t fd = rt_socket_fd_for_reactor(s);
    rt_reactor_register(reactor, fd, 0);
    int32_t rc = rt_reactor_submit_read(reactor, fd, buf, (uint32_t)bufferSize, 0, compl);
    if (rc != 0) {
        free(compl);
        rt_task_release(task);
        free(buf);
        return NULL;
    }
    return task;
}

/* 字节面异步接收：写入调用方 buffer（RtIoCompletion 不持有 buffer，仅记录
 * buf/buf_size 供完成处理；buffer 归调用方所有，Task 完成前须保持有效）。
 * 用于 TLS 密文（含 0x00）等二进制面真异步读。 */
void* rt_socket_receive_bytes_async(void* handle, void* buffer, int32_t bufferSize) {
    RtSocket* s = (RtSocket*)handle;
    if (!s || s->closed || !buffer || bufferSize <= 0) return NULL;

    void* loop = rt_event_loop_current();
    void* reactor = loop ? rt_event_loop_get_reactor(loop) : NULL;
    if (!reactor) return NULL;

    rt_socket_set_nonblocking_internal(s);

    RtTask* task = rt_task_alloc();
    if (!task) return NULL;
    task->status = RT_TASK_PENDING;

    RtIoCompletion* compl = (RtIoCompletion*)calloc(1, sizeof(RtIoCompletion));
    if (!compl) {
        rt_task_release(task);
        return NULL;
    }
    compl->task = task;
    compl->op_type = RT_IO_OP_READ_BYTES;
    compl->buf = buffer;
    compl->buf_size = bufferSize;

    int32_t fd = rt_socket_fd_for_reactor(s);
    rt_reactor_register(reactor, fd, 0);
    int32_t rc = rt_reactor_submit_read(reactor, fd, buffer, (uint32_t)bufferSize, 0, compl);
    if (rc != 0) {
        free(compl);
        rt_task_release(task);
        return NULL;
    }
    return task;
}

/* IO 完成事件处理器：把 result 写回 Task，触发 waker，释放上下文。
 * 文件 async（op_type >= RT_IO_OP_FILE_BASE）经可插拔指针 g_rt_io_file_completion
 * 转发到 rt_file.c 的 rt_file_io_completion_complete（解耦跨域硬链）；未注册时
 * 走安全 no-op（int_result=0 + 释放）。网络 op_type 在此处理。 */
void (*g_rt_io_file_completion)(void* user_data, int32_t result) = NULL;

void rt_io_completion_complete(void* user_data, int32_t result) {
    RtIoCompletion* compl = (RtIoCompletion*)user_data;
    if (!compl || !compl->task) return;
    if (compl->op_type >= RT_IO_OP_FILE_BASE) {
        if (g_rt_io_file_completion) {
            g_rt_io_file_completion(user_data, result);
        } else {
            /* rt_file.c 未链接：安全 no-op（标记失败 + 完成 + 释放） */
            compl->task->int_result = 0;
            rt_task_complete(compl->task);
            if (compl->buf) free(compl->buf);
            free(compl);
        }
        return;
    }

    RtTask* task = compl->task;
    switch (compl->op_type) {
        case RT_IO_OP_CONNECT:
            /* result 0=成功（IOCP connect 完成时 bytes_transferred=0），
             * <0=错误。写 int_result 1=成功/0=失败。 */
            task->int_result = (result >= 0) ? 1 : 0;
            break;
        case RT_IO_OP_ACCEPT: {
            /* result = 新 accept socket 的 fd（int32_t）。
             * 包装为 RtSocket* 写 ptr_result。 */
            if (result > 0) {
                /* RFC 050 M-a：opaque 统一头试点（accept 包装对象同族）。 */
                RtSocket* accept_sock = (RtSocket*)rt_obj_alloc_opaque(sizeof(RtSocket));
                if (accept_sock) {
#if defined(_WIN32)
                    accept_sock->fd = (SOCKET)(intptr_t)result;
#else
                    accept_sock->fd = result;
#endif
                    accept_sock->closed = 0;
                    task->ptr_result = accept_sock;
                    task->int_result = 1;
                } else {
                    task->int_result = 0;
                }
            } else {
                task->int_result = 0;
            }
            break;
        }
        case RT_IO_OP_READ:
            /* result = 字节数（0=EOF，<0=错误）。
             * buf 已由 Reactor 填充，转为 NUL 终止 string 写 ptr_result。
             * int_result 存原始字节数。 */
            task->int_result = result;
            if (result > 0 && compl->buf) {
                /* 确保 NUL 终止（buffer 分配时已 +1 并 calloc 清零） */
                ((char*)compl->buf)[result] = '\0';
                task->ptr_result = compl->buf;
                compl->buf = NULL;  /* 所有权转移到 Task，不再释放 */
            }
            break;
        case RT_IO_OP_READ_BYTES:
            /* 字节面读：buffer 归调用方所有，仅写回字节数；不转移所有权、
             * 不 NUL 终止。置空 compl->buf 防末尾 free 误释放调用方 buffer。
             * result = 字节数（0=EOF，<0=错误）。 */
            task->int_result = result;
            compl->buf = NULL;
            break;
        case RT_IO_OP_WRITE:
            /* result = 字节数 */
            task->int_result = result;
            break;
    }

    /* 标记 READY + 触发 waker（将 outer Task 移入就绪队列） */
    rt_task_complete(task);

    /* 释放 completion 上下文（buf 所所有权可能已转移） */
    if (compl->buf) free(compl->buf);
    free(compl);
}

/* ---- DNS ----------------------------------------------------------------- */

void* rt_dns_resolve(const char* host) {
    if (!host || host[0] == '\0') return NULL;

    struct addrinfo hints, *result = NULL;
    memset(&hints, 0, sizeof(hints));
    hints.ai_family = AF_UNSPEC;
    hints.ai_socktype = SOCK_STREAM;

    if (getaddrinfo((const char*)host, NULL, &hints, &result) != 0) {
        return NULL;
    }

    /* Resolve the first address to a presentation-format string. */
    char ip_str[INET6_ADDRSTRLEN];
    const char* resolved = NULL;

    for (struct addrinfo* rp = result; rp != NULL; rp = rp->ai_next) {
        if (rp->ai_family == AF_INET) {
            struct sockaddr_in* addr = (struct sockaddr_in*)rp->ai_addr;
            if (inet_ntop(AF_INET, &addr->sin_addr, ip_str, sizeof(ip_str))) {
                resolved = ip_str;
                break;
            }
        } else if (rp->ai_family == AF_INET6) {
            struct sockaddr_in6* addr = (struct sockaddr_in6*)rp->ai_addr;
            if (inet_ntop(AF_INET6, &addr->sin6_addr, ip_str, sizeof(ip_str))) {
                resolved = ip_str;
                break;
            }
        }
    }

    char* out = NULL;
    if (resolved) {
        size_t len = strlen(resolved);
        out = (char*)malloc(len + 1);
        if (out) memcpy(out, resolved, len + 1);
    }
    freeaddrinfo(result);
    return (void*)out;
}

void* rt_dns_get_host_name(void) {
    char hostname[256];
    if (gethostname(hostname, sizeof(hostname)) != 0) {
        return NULL;
    }
    hostname[sizeof(hostname) - 1] = '\0';
    size_t len = strlen(hostname);
    char* out = (char*)malloc(len + 1);
    if (!out) return NULL;
    memcpy(out, hostname, len + 1);
    return (void*)out;
}

void* rt_dns_resolve_all(const char* host) {
    if (!host || host[0] == '\0') return NULL;

    struct addrinfo hints, *result = NULL;
    memset(&hints, 0, sizeof(hints));
    hints.ai_family = AF_UNSPEC;
    hints.ai_socktype = SOCK_STREAM;

    if (getaddrinfo((const char*)host, NULL, &hints, &result) != 0) {
        return NULL;
    }

    /* Collect all unique addresses into a space-separated string. */
    char* buf = (char*)malloc(4096);
    if (!buf) { freeaddrinfo(result); return NULL; }
    buf[0] = '\0';

    for (struct addrinfo* rp = result; rp != NULL; rp = rp->ai_next) {
        char ip_str[INET6_ADDRSTRLEN];
        const char* resolved = NULL;
        if (rp->ai_family == AF_INET) {
            struct sockaddr_in* addr = (struct sockaddr_in*)rp->ai_addr;
            if (inet_ntop(AF_INET, &addr->sin_addr, ip_str, sizeof(ip_str))) {
                resolved = ip_str;
            }
        } else if (rp->ai_family == AF_INET6) {
            struct sockaddr_in6* addr = (struct sockaddr_in6*)rp->ai_addr;
            if (inet_ntop(AF_INET6, &addr->sin6_addr, ip_str, sizeof(ip_str))) {
                resolved = ip_str;
            }
        }
        if (resolved) {
            /* Avoid duplicates */
            if (strstr(buf, resolved) == NULL) {
                if (buf[0] != '\0') { strcat(buf, " "); }
                strcat(buf, resolved);
            }
        }
    }
    freeaddrinfo(result);
    if (buf[0] == '\0') { free(buf); return NULL; }
    return (void*)buf;
}
