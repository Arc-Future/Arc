// poll 回退 Reactor 后端实现（RFC 009 M1 嵌入式回退）。
//
// poll 是 POSIX 标准的 IO 多路复用，性能最低但兼容性最广。
// 用于不支持 io_uring/kqueue 的嵌入式平台。
//
// 限制：
//   - 不支持异步文件 IO（poll 仅支持 socket/pipe）
//   - 不支持批量提交（每次 submit 直接发起阻塞 IO）
//   - 不支持零拷贝缓冲池
//
// 实现：submit_* 直接同步执行 IO（阻塞），结果暂存到完成队列；
//       poll 从完成队列取出事件。这是"假异步"——仅用于功能兼容。

#if !defined(__linux__) && !defined(_WIN32) && !defined(__APPLE__) && !defined(__FreeBSD__)

#include "rt_abi.h"
#include <stdlib.h>
#include <string.h>
#include <poll.h>
#include <unistd.h>
#include <fcntl.h>
#include <errno.h>

/* ---- poll Reactor 内部结构 ---- */

typedef struct RtPollEvent {
    void*    user_data;
    int32_t  result;
    int32_t  fd;
    uint32_t flags;
} RtPollEvent;

typedef struct RtReactorPoll {
    /* 已注册的 fd 列表 */
    int32_t*  fds;
    uint32_t* events;
    int32_t   fd_count;
    int32_t   fd_capacity;
    /* 同步完成队列（submit 直接执行，结果暂存） */
    RtPollEvent* pending;
    int32_t pending_head;
    int32_t pending_tail;
    int32_t pending_capacity;
} RtReactorPoll;

static void rt_poll_ensure_capacity(RtReactorPoll* r, int32_t needed) {
    if (r->fd_capacity >= needed) return;
    int32_t new_cap = r->fd_capacity * 2;
    if (new_cap < 16) new_cap = 16;
    while (new_cap < needed) new_cap *= 2;
    r->fds = (int32_t*)realloc(r->fds, new_cap * sizeof(int32_t));
    r->events = (uint32_t*)realloc(r->events, new_cap * sizeof(uint32_t));
    r->fd_capacity = new_cap;
}

static void rt_poll_push_pending(RtReactorPoll* r, RtPollEvent* ev) {
    int32_t next = (r->pending_tail + 1) % r->pending_capacity;
    if (next == r->pending_head) {
        /* 扩容 */
        int32_t new_cap = r->pending_capacity * 2;
        RtPollEvent* new_pending = (RtPollEvent*)malloc(new_cap * sizeof(RtPollEvent));
        int32_t n = 0;
        while (r->pending_head != r->pending_tail) {
            new_pending[n++] = r->pending[r->pending_head];
            r->pending_head = (r->pending_head + 1) % r->pending_capacity;
        }
        free(r->pending);
        r->pending = new_pending;
        r->pending_head = 0;
        r->pending_tail = n;
        r->pending_capacity = new_cap;
        next = (r->pending_tail + 1) % r->pending_capacity;
    }
    r->pending[r->pending_tail] = *ev;
    r->pending_tail = next;
}

/* ---- impl 接口实现 ---- */

void* rt_reactor_impl_create(uint32_t flags) {
    (void)flags; /* poll 不支持 SQPOLL */
    RtReactorPoll* r = (RtReactorPoll*)calloc(1, sizeof(RtReactorPoll));
    if (!r) return NULL;
    r->fd_capacity = 16;
    r->fds = (int32_t*)malloc(r->fd_capacity * sizeof(int32_t));
    r->events = (uint32_t*)malloc(r->fd_capacity * sizeof(uint32_t));
    r->fd_count = 0;
    r->pending_capacity = 16;
    r->pending = (RtPollEvent*)malloc(r->pending_capacity * sizeof(RtPollEvent));
    r->pending_head = 0;
    r->pending_tail = 0;
    return r;
}

void rt_reactor_impl_destroy(void* backend) {
    RtReactorPoll* r = (RtReactorPoll*)backend;
    if (!r) return;
    free(r->fds);
    free(r->events);
    free(r->pending);
    free(r);
}

int32_t rt_reactor_impl_register(void* backend, int32_t fd, uint32_t events) {
    RtReactorPoll* r = (RtReactorPoll*)backend;
    if (!r) return -1;
    rt_poll_ensure_capacity(r, r->fd_count + 1);
    r->fds[r->fd_count] = fd;
    r->events[r->fd_count] = events;
    r->fd_count++;
    return 0;
}

int32_t rt_reactor_impl_modify(void* backend, int32_t fd, uint32_t events) {
    RtReactorPoll* r = (RtReactorPoll*)backend;
    if (!r) return -1;
    for (int32_t i = 0; i < r->fd_count; i++) {
        if (r->fds[i] == fd) {
            r->events[i] = events;
            return 0;
        }
    }
    return -1;
}

int32_t rt_reactor_impl_unregister(void* backend, int32_t fd) {
    RtReactorPoll* r = (RtReactorPoll*)backend;
    if (!r) return -1;
    for (int32_t i = 0; i < r->fd_count; i++) {
        if (r->fds[i] == fd) {
            /* 移除：用最后一个覆盖 */
            r->fds[i] = r->fds[r->fd_count - 1];
            r->events[i] = r->events[r->fd_count - 1];
            r->fd_count--;
            return 0;
        }
    }
    return -1;
}

/* poll 后端：submit_* 同步执行（假异步），结果暂存到 pending 队列 */

int32_t rt_reactor_impl_submit_read(void* backend, int32_t fd, void* buf,
                                     uint32_t len, uint64_t offset, void* user_data) {
    RtReactorPoll* r = (RtReactorPoll*)backend;
    if (!r) return -1;
    (void)offset;  /* poll 后端不支持 offset，假设是 socket */

    ssize_t n = read(fd, buf, len);
    RtPollEvent ev = {0};
    ev.user_data = user_data;
    ev.fd = fd;
    ev.result = (n < 0) ? -errno : (int32_t)n;
    rt_poll_push_pending(r, &ev);
    return 0;
}

int32_t rt_reactor_impl_submit_write(void* backend, int32_t fd, const void* buf,
                                      uint32_t len, uint64_t offset, void* user_data) {
    RtReactorPoll* r = (RtReactorPoll*)backend;
    if (!r) return -1;
    (void)offset;

    ssize_t n = write(fd, buf, len);
    RtPollEvent ev = {0};
    ev.user_data = user_data;
    ev.fd = fd;
    ev.result = (n < 0) ? -errno : (int32_t)n;
    rt_poll_push_pending(r, &ev);
    return 0;
}

int32_t rt_reactor_impl_submit_accept(void* backend, int32_t listen_fd, void* user_data) {
    RtReactorPoll* r = (RtReactorPoll*)backend;
    if (!r) return -1;
    int32_t client_fd = (int32_t)accept(listen_fd, NULL, NULL);
    RtPollEvent ev = {0};
    ev.user_data = user_data;
    ev.fd = listen_fd;
    ev.result = (client_fd < 0) ? -errno : client_fd;
    rt_poll_push_pending(r, &ev);
    return 0;
}

int32_t rt_reactor_impl_submit_connect(void* backend, int32_t fd,
                                        const void* addr, uint32_t addr_len, void* user_data) {
    RtReactorPoll* r = (RtReactorPoll*)backend;
    if (!r) return -1;
    int32_t ret = connect(fd, (const struct sockaddr*)addr, addr_len);
    RtPollEvent ev = {0};
    ev.user_data = user_data;
    ev.fd = fd;
    ev.result = (ret < 0) ? -errno : 0;
    rt_poll_push_pending(r, &ev);
    return 0;
}

int32_t rt_reactor_impl_flush(void* backend) {
    (void)backend;
    return 0;
}

int32_t rt_reactor_impl_poll(void* backend, RtIoEvent* events, int32_t max_events,
                              int32_t timeout_ms) {
    RtReactorPoll* r = (RtReactorPoll*)backend;
    if (!r) return 0;

    int32_t n = 0;
    /* 优先返回 pending 队列中的事件 */
    while (n < max_events && r->pending_head != r->pending_tail) {
        RtPollEvent* pe = &r->pending[r->pending_head];
        events[n].user_data = pe->user_data;
        events[n].fd = pe->fd;
        events[n].result = pe->result;
        events[n].flags = pe->flags;
        r->pending_head = (r->pending_head + 1) % r->pending_capacity;
        n++;
    }

    if (n > 0) return n;

    /* 无 pending 事件，用 poll 等待 fd 就绪 */
    if (r->fd_count == 0 || timeout_ms == 0) return 0;

    struct pollfd* pfds = (struct pollfd*)malloc(r->fd_count * sizeof(struct pollfd));
    if (!pfds) return -1;
    for (int32_t i = 0; i < r->fd_count; i++) {
        pfds[i].fd = r->fds[i];
        pfds[i].events = 0;
        if (r->events[i] & RT_REACTOR_READABLE) pfds[i].events |= POLLIN;
        if (r->events[i] & RT_REACTOR_WRITABLE) pfds[i].events |= POLLOUT;
        pfds[i].revents = 0;
    }

    int32_t ready = poll(pfds, (nfds_t)r->fd_count, timeout_ms);
    if (ready > 0) {
        for (int32_t i = 0; i < r->fd_count && n < max_events; i++) {
            if (pfds[i].revents) {
                events[n].user_data = NULL;
                events[n].fd = pfds[i].fd;
                events[n].result = 0;
                events[n].flags = 0;
                if (pfds[i].revents & POLLIN) events[n].flags |= RT_REACTOR_READABLE;
                if (pfds[i].revents & POLLOUT) events[n].flags |= RT_REACTOR_WRITABLE;
                if (pfds[i].revents & POLLERR) events[n].flags |= RT_REACTOR_ERROR;
                if (pfds[i].revents & POLLHUP) events[n].flags |= RT_REACTOR_HANGUP;
                n++;
            }
        }
    }
    free(pfds);
    return n;
}

/* RFC 009 M6: 跨线程唤醒（预留）。poll 后端需 pipe/eventfd 注册到 fd 集才能
 * 唤醒阻塞的 poll()——属后续里程碑；当前 no-op 由 EventLoop 的 ≤100ms 轮询
 * 兜底（功能性正确，唤醒延迟 ≤100ms）。 */
void rt_reactor_impl_wake(void* backend) {
    (void)backend;
}

int32_t rt_reactor_impl_register_buffers(void* backend, const void** buffers,
                                          const uint32_t* lengths, int32_t n) {
    (void)backend; (void)buffers; (void)lengths; (void)n;
    return 0;
}

const char* rt_reactor_impl_backend_name(void) {
    return "poll";
}

void rt_reactor_impl_set_link_flag(void* backend, int32_t enable) {
    (void)backend; (void)enable; /* poll 不支持链式操作 */
}

int32_t rt_reactor_impl_submit_timeout(void* backend, uint64_t timeout_ns, void* user_data) {
    (void)backend; (void)timeout_ns; (void)user_data;
    return -1; /* poll 不支持 timeout 提交 */
}

#endif /* 回退平台 */
