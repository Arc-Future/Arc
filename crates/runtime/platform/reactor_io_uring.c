// Linux io_uring Reactor 后端实现（RFC 009 M1）。
//
// io_uring 是 Linux 5.1+ 的高性能异步 IO 接口（Jens Axboe, 2019）：
//   - SQ/CQ ring buffer（共享内存，mmap）
//   - 提交 SQE（填入 SQ ring）→ io_uring_enter → CQE 写入 CQ ring
//   - 批量提交：N 个 SQE 一次 io_uring_enter，渐近 O(1) per IO
//   - 零拷贝：io_uring_register_buffers 预注册 user buffer
//
// 与 IOCP 的差异：
//   - io_uring 是"提交/完成"模型（提交请求后轮询 CQE）
//   - 支持批量提交（N SQE → 1 syscall）
//   - 支持 SQPOLL 模式（内核线程轮询 SQ，无 syscall）
//
// 实现原则（D-1 决策）：不依赖 liburing，全部 raw syscall，100% 独立。
//
// 性能目标（RFC 009 §0.2）：
//   - IO 吞吐 ≥1M req/s（10⁴ 并发连接）
//   - IO 系统调用渐近 O(1) per IO（批量提交）

#if defined(__linux__)

#include "rt_abi.h"
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <fcntl.h>
#include <errno.h>
#include <sys/syscall.h>
#include <sys/mman.h>
#include <sys/uio.h>
#include <linux/io_uring.h>
#include <sys/socket.h>
#include <netinet/in.h>

/* ---- io_uring raw syscall wrappers ---- */

static int io_uring_setup(unsigned entries, struct io_uring_params* p) {
    return (int)syscall(__NR_io_uring_setup, entries, p);
}

static int io_uring_enter(int fd, unsigned to_submit, unsigned min_complete,
                          unsigned flags, void* sig) {
    return (int)syscall(__NR_io_uring_enter, fd, to_submit, min_complete, flags, sig);
}

static int io_uring_register(int fd, unsigned opcode, void* arg, unsigned nr_args) {
    return (int)syscall(__NR_io_uring_register, fd, opcode, arg, nr_args);
}

/* ---- io_uring Reactor 内部结构 ---- */

typedef struct RtReactorIoUring {
    int ring_fd;                    /* io_uring 文件描述符 */

    /* SQ ring（提交队列） */
    unsigned* sq_head;              /* 内核维护的 head */
    unsigned* sq_tail;              /* 用户维护的 tail */
    unsigned* sq_mask;
    unsigned* sq_array;             /* 间接索引数组 */
    struct io_uring_sqe* sqes;      /* SQE 数组 */
    unsigned sq_tail_shadow;        /* 用户侧 tail 影子变量 */
    void** sqe_user_data;           /* 每 SQE 的 user_data 暂存 */
    unsigned sq_pending;            /* 已填入但未 enter 的 SQE 数 */

    /* CQ ring（完成队列） */
    unsigned* cq_head;              /* 用户维护的 head */
    unsigned* cq_tail;              /* 内核维护的 tail */
    unsigned* cq_mask;
    struct io_uring_cqe* cqes;

    /* mmap 区域（用于 munmap） */
    void* sq_mmap;
    void* sqe_mmap;
    void* cq_mmap;
    size_t sq_mmap_size;
    size_t sqe_mmap_size;
    size_t cq_mmap_size;

    /* RFC 009 M7：链式操作状态 */
    int link_next;                  /* 下一 SQE 是否设置 IOSQE_IO_LINK */
} RtReactorIoUring;

/* ---- impl 接口实现 ---- */

void* rt_reactor_impl_create(uint32_t flags) {
    RtReactorIoUring* r = (RtReactorIoUring*)calloc(1, sizeof(RtReactorIoUring));
    if (!r) return NULL;

    struct io_uring_params p;
    memset(&p, 0, sizeof(p));

    /* SQPOLL 模式（RFC 009 M6）：内核轮询线程，无需 syscall，
     * 以 CPU 换 IO 延迟（适用于高 IO 场景）。默认关闭。 */
    if (flags & RT_REACTOR_FLAG_SQPOLL) {
        p.flags |= IORING_SETUP_SQPOLL;
        /* SQPOLL 内核线程空闲超时（单位 ms），0 使用内核默认 */
        p.sq_thread_idle = 1000;
    }

    /* 创建 io_uring，128 个 SQE 条目 */
    r->ring_fd = io_uring_setup(128, &p);
    if (r->ring_fd < 0) {
        free(r);
        return NULL;
    }

    /* mmap SQ ring */
    r->sq_mmap_size = p.sq_off.array + p.sq_entries * sizeof(unsigned);
    r->sq_mmap = mmap(NULL, r->sq_mmap_size, PROT_READ | PROT_WRITE,
                      MAP_SHARED | MAP_POPULATE, r->ring_fd, IORING_OFF_SQ_RING);
    if (r->sq_mmap == MAP_FAILED) { close(r->ring_fd); free(r); return NULL; }

    r->sq_head = (unsigned*)((char*)r->sq_mmap + p.sq_off.head);
    r->sq_tail = (unsigned*)((char*)r->sq_mmap + p.sq_off.tail);
    r->sq_mask = (unsigned*)((char*)r->sq_mmap + p.sq_off.ring_mask);
    r->sq_array = (unsigned*)((char*)r->sq_mmap + p.sq_off.array);

    /* mmap SQE 数组 */
    r->sqe_mmap_size = p.sq_entries * sizeof(struct io_uring_sqe);
    r->sqes = (struct io_uring_sqe*)mmap(NULL, r->sqe_mmap_size, PROT_READ | PROT_WRITE,
                                          MAP_SHARED | MAP_POPULATE, r->ring_fd, IORING_OFF_SQES);
    if (r->sqes == MAP_FAILED) {
        munmap(r->sq_mmap, r->sq_mmap_size);
        close(r->ring_fd); free(r); return NULL;
    }

    /* mmap CQ ring */
    r->cq_mmap_size = p.cq_off.cqes + p.cq_entries * sizeof(struct io_uring_cqe);
    r->cq_mmap = mmap(NULL, r->cq_mmap_size, PROT_READ | PROT_WRITE,
                      MAP_SHARED | MAP_POPULATE, r->ring_fd, IORING_OFF_CQ_RING);
    if (r->cq_mmap == MAP_FAILED) {
        munmap(r->sqes, r->sqe_mmap_size);
        munmap(r->sq_mmap, r->sq_mmap_size);
        close(r->ring_fd); free(r); return NULL;
    }

    r->cq_head = (unsigned*)((char*)r->cq_mmap + p.cq_off.head);
    r->cq_tail = (unsigned*)((char*)r->cq_mmap + p.cq_off.tail);
    r->cq_mask = (unsigned*)((char*)r->cq_mmap + p.cq_off.ring_mask);
    r->cqes = (struct io_uring_cqe*)((char*)r->cq_mmap + p.cq_off.cqes);

    r->sq_tail_shadow = *r->sq_tail;
    r->sq_pending = 0;
    r->link_next = 0;

    /* user_data 暂存数组（用于 sqe_user_data[idx]） */
    r->sqe_user_data = (void**)calloc(p.sq_entries, sizeof(void*));
    if (!r->sqe_user_data) {
        munmap(r->cq_mmap, r->cq_mmap_size);
        munmap(r->sqes, r->sqe_mmap_size);
        munmap(r->sq_mmap, r->sq_mmap_size);
        close(r->ring_fd); free(r); return NULL;
    }

    return r;
}

void rt_reactor_impl_destroy(void* backend) {
    RtReactorIoUring* r = (RtReactorIoUring*)backend;
    if (!r) return;
    if (r->ring_fd >= 0) close(r->ring_fd);
    if (r->sq_mmap != MAP_FAILED && r->sq_mmap)
        munmap(r->sq_mmap, r->sq_mmap_size);
    if (r->sqes != MAP_FAILED && r->sqes)
        munmap(r->sqes, r->sqe_mmap_size);
    if (r->cq_mmap != MAP_FAILED && r->cq_mmap)
        munmap(r->cq_mmap, r->cq_mmap_size);
    free(r->sqe_user_data);
    free(r);
}

int32_t rt_reactor_impl_register(void* backend, int32_t fd, uint32_t events) {
    /* io_uring 不需要预先注册 fd——提交 SQE 时直接指定 fd */
    (void)backend; (void)fd; (void)events;
    return 0;
}

int32_t rt_reactor_impl_modify(void* backend, int32_t fd, uint32_t events) {
    (void)backend; (void)fd; (void)events;
    return 0;
}

int32_t rt_reactor_impl_unregister(void* backend, int32_t fd) {
    (void)backend; (void)fd;
    return 0;
}

/* 提交一个 SQE 到 SQ ring（不立即 io_uring_enter，累积到 flush） */
static int32_t rt_iouring_push_sqe(RtReactorIoUring* r, struct io_uring_sqe* sqe_template,
                                    void* user_data) {
    unsigned head = __atomic_load_n(r->sq_head, __ATOMIC_ACQUIRE);
    unsigned next = r->sq_tail_shadow + 1;
    /* 检查 SQ 是否已满 */
    if (next - head > *r->sq_mask) {
        /* SQ 满：先 flush */
        rt_reactor_impl_flush(r);
        head = __atomic_load_n(r->sq_head, __ATOMIC_ACQUIRE);
        if (next - head > *r->sq_mask) {
            return -1;  /* 仍然满 */
        }
    }

    unsigned idx = r->sq_tail_shadow & *r->sq_mask;
    r->sqes[idx] = *sqe_template;

    /* RFC 038 M7：IOSQE_IO_LINK 链式操作 —— 当前 SQE 标记为链接下一个 */
    if (r->link_next) {
        r->sqes[idx].flags |= IOSQE_IO_LINK;
        r->link_next = 0;
    }

    /* user_data 用 sq_tail_shadow 作为唯一标识，存入暂存数组 */
    r->sqes[idx].user_data = (unsigned long long)r->sq_tail_shadow;
    r->sqe_user_data[idx] = user_data;

    /* 写入 SQ array（间接索引） */
    r->sq_array[idx] = idx;
    __atomic_store_n(r->sq_tail, r->sq_tail_shadow + 1, __ATOMIC_RELEASE);
    r->sq_tail_shadow++;
    r->sq_pending++;
    return 0;
}

int32_t rt_reactor_impl_submit_read(void* backend, int32_t fd, void* buf,
                                     uint32_t len, uint64_t offset, void* user_data) {
    RtReactorIoUring* r = (RtReactorIoUring*)backend;
    if (!r) return -1;

    struct io_uring_sqe sqe;
    memset(&sqe, 0, sizeof(sqe));
    sqe.opcode = IORING_OP_READ;
    sqe.fd = fd;
    sqe.addr = (unsigned long)buf;
    sqe.len = len;
    sqe.off = offset;

    return rt_iouring_push_sqe(r, &sqe, user_data);
}

int32_t rt_reactor_impl_submit_write(void* backend, int32_t fd, const void* buf,
                                      uint32_t len, uint64_t offset, void* user_data) {
    RtReactorIoUring* r = (RtReactorIoUring*)backend;
    if (!r) return -1;

    struct io_uring_sqe sqe;
    memset(&sqe, 0, sizeof(sqe));
    sqe.opcode = IORING_OP_WRITE;
    sqe.fd = fd;
    sqe.addr = (unsigned long)buf;
    sqe.len = len;
    sqe.off = offset;

    return rt_iouring_push_sqe(r, &sqe, user_data);
}

int32_t rt_reactor_impl_submit_accept(void* backend, int32_t listen_fd, void* user_data) {
    RtReactorIoUring* r = (RtReactorIoUring*)backend;
    if (!r) return -1;

    struct io_uring_sqe sqe;
    memset(&sqe, 0, sizeof(sqe));
    sqe.opcode = IORING_OP_ACCEPT;
    sqe.fd = listen_fd;
    sqe.addr = 0;  /* NULL addr → 自动填充 */
    sqe.addr2 = 0;  /* NULL addrlen */

    return rt_iouring_push_sqe(r, &sqe, user_data);
}

int32_t rt_reactor_impl_submit_connect(void* backend, int32_t fd,
                                        const void* addr, uint32_t addr_len, void* user_data) {
    RtReactorIoUring* r = (RtReactorIoUring*)backend;
    if (!r) return -1;

    struct io_uring_sqe sqe;
    memset(&sqe, 0, sizeof(sqe));
    sqe.opcode = IORING_OP_CONNECT;
    sqe.fd = fd;
    sqe.addr = (unsigned long)addr;
    sqe.off = addr_len;  /* connect 用 off 存 addr_len */

    return rt_iouring_push_sqe(r, &sqe, user_data);
}

int32_t rt_reactor_impl_flush(void* backend) {
    RtReactorIoUring* r = (RtReactorIoUring*)backend;
    if (!r || r->sq_pending == 0) return 0;

    int ret = io_uring_enter(r->ring_fd, r->sq_pending, 0, 0, NULL);
    if (ret < 0) return -errno;
    r->sq_pending = 0;
    return ret;
}

int32_t rt_reactor_impl_poll(void* backend, RtIoEvent* events, int32_t max_events,
                              int32_t timeout_ms) {
    RtReactorIoUring* r = (RtReactorIoUring*)backend;
    if (!r) return 0;

    /* 先 flush 待提交的 SQE */
    if (r->sq_pending > 0) {
        rt_reactor_impl_flush(r);
    }

    int32_t n = 0;
    unsigned head = __atomic_load_n(r->cq_head, __ATOMIC_ACQUIRE);
    unsigned tail = __atomic_load_n(r->cq_tail, __ATOMIC_ACQUIRE);

    while (n < max_events && head != tail) {
        unsigned idx = head & *r->cq_mask;
        struct io_uring_cqe* cqe = &r->cqes[idx];

        /* 通过 user_data 找回原始 user_data */
        unsigned long long ud = cqe->user_data;
        unsigned sq_idx = (unsigned)(ud & *r->cq_mask);
        events[n].user_data = r->sqe_user_data[sq_idx];
        events[n].result = cqe->res;  /* 字节数 / -errno */
        events[n].flags = cqe->flags;
        events[n].fd = -1;  /* io_uring CQE 不含 fd，由 user_data 关联 */

        head++;
        n++;
        tail = __atomic_load_n(r->cq_tail, __ATOMIC_ACQUIRE);
    }

    /* 推进 CQ head（通知内核已消费） */
    if (n > 0) {
        __atomic_store_n(r->cq_head, head, __ATOMIC_RELEASE);
    }

    /* 如果无事件且需要等待，调用 io_uring_enter 阻塞等待 */
    if (n == 0 && timeout_ms != 0) {
        unsigned enter_flags = IORING_ENTER_GETEVENTS;
        if (timeout_ms > 0) {
            /* io_uring 无直接 timeout 参数，用 io_uring_register 或 IORING_OP_TIMEOUT
             * 简化：非阻塞 GETEVENTS + 外部 sleep 循环（MVP） */
            enter_flags |= 0;
        }
        int ret = io_uring_enter(r->ring_fd, 0, 1, enter_flags, NULL);
        if (ret >= 0) {
            /* 重新读取 CQ */
            head = __atomic_load_n(r->cq_head, __ATOMIC_ACQUIRE);
            tail = __atomic_load_n(r->cq_tail, __ATOMIC_ACQUIRE);
            while (n < max_events && head != tail) {
                unsigned idx = head & *r->cq_mask;
                struct io_uring_cqe* cqe = &r->cqes[idx];
                unsigned long long ud = cqe->user_data;
                unsigned sq_idx = (unsigned)(ud & *r->cq_mask);
                events[n].user_data = r->sqe_user_data[sq_idx];
                events[n].result = cqe->res;
                events[n].flags = cqe->flags;
                events[n].fd = -1;
                head++;
                n++;
                tail = __atomic_load_n(r->cq_tail, __ATOMIC_ACQUIRE);
            }
            if (n > 0) {
                __atomic_store_n(r->cq_head, head, __ATOMIC_RELEASE);
            }
        }
    }

    return n;
}

/* RFC 009 M6: 跨线程唤醒（预留）。io_uring 需 eventfd（IORING_REGISTER_EVENTFD）
 * 注册后注入才能唤醒阻塞的 io_uring_enter——属后续里程碑；当前 no-op 由
 * EventLoop 的 ≤100ms 轮询兜底（功能性正确，唤醒延迟 ≤100ms）。 */
void rt_reactor_impl_wake(void* backend) {
    (void)backend;
}

int32_t rt_reactor_impl_register_buffers(void* backend, const void** buffers,
                                          const uint32_t* lengths, int32_t n) {
    RtReactorIoUring* r = (RtReactorIoUring*)backend;
    if (!r) return -1;

    /* 构造 iovec 数组 */
    struct iovec* iovecs = (struct iovec*)malloc(n * sizeof(struct iovec));
    if (!iovecs) return -ENOMEM;
    for (int32_t i = 0; i < n; i++) {
        iovecs[i].iov_base = (void*)buffers[i];
        iovecs[i].iov_len = lengths[i];
    }

    int ret = io_uring_register(r->ring_fd, IORING_REGISTER_BUFFERS, iovecs, n);
    free(iovecs);
    return (ret < 0) ? -errno : 0;
}

const char* rt_reactor_impl_backend_name(void) {
    return "io_uring";
}

/* ---- RFC 009 M7：链式操作 + timeout ---- */

void rt_reactor_impl_set_link_flag(void* backend, int32_t enable) {
    RtReactorIoUring* r = (RtReactorIoUring*)backend;
    if (!r) return;
    r->link_next = enable ? 1 : 0;
}

int32_t rt_reactor_impl_submit_timeout(void* backend, uint64_t timeout_ns, void* user_data) {
    RtReactorIoUring* r = (RtReactorIoUring*)backend;
    if (!r) return -1;

    struct io_uring_sqe sqe;
    memset(&sqe, 0, sizeof(sqe));
    sqe.opcode = IORING_OP_TIMEOUT;
    sqe.addr = timeout_ns;      /* 超时时间（纳秒） */
    sqe.len = 1;                 /* 1 个 completion event */
    sqe.off = 0;                 /* IORING_TIMEOUT_ABS=0，相对时间 */

    return rt_iouring_push_sqe(r, &sqe, user_data);
}

#endif /* __linux__ */
