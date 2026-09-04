// macOS / FreeBSD kqueue Reactor 后端实现（RFC 009 M5）。
//
// kqueue 是 BSD 系列的 IO 多路复用机制：
//   - 创建 kqueue（kqueue()）
//   - 注册/修改 fd 监听事件（kevent + EV_ADD/EV_DELETE）
//   - 轮询就绪事件（kevent + 返回 changelist）
//
// 与 io_uring 的差异：
//   - kqueue 是"就绪"模型（fd 就绪后通知），io_uring 是"完成"模型
//   - kqueue 不支持批量提交 IO 操作（仅批量注册/修改监听）
//   - kqueue 支持 timer/filter/vnode 等多种事件类型
//
// 性能特征：
//   - O(1) 注册/修改/删除
//   - 批量获取就绪事件（kevent 一次返回多个）
//   - 适合高并发连接场景

#if defined(__APPLE__) || defined(__FreeBSD__)

#include "rt_abi.h"
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <fcntl.h>
#include <errno.h>
#include <sys/types.h>
#include <sys/event.h>
#include <sys/socket.h>
#include <netinet/in.h>

/* ---- kqueue Reactor 内部结构 ---- */

typedef struct RtReactorKqueue {
    int kq;  /* kqueue fd */
} RtReactorKqueue;

/* ---- impl 接口实现 ---- */

void* rt_reactor_impl_create(uint32_t flags) {
    (void)flags; /* kqueue 不支持 SQPOLL */
    RtReactorKqueue* r = (RtReactorKqueue*)calloc(1, sizeof(RtReactorKqueue));
    if (!r) return NULL;
    r->kq = kqueue();
    if (r->kq < 0) {
        free(r);
        return NULL;
    }
    /* RFC 009 M6: 注册 EVFILT_USER 用户事件（ident=0）——跨线程唤醒哨兵。
     * EV_CLEAR 使 NOTE_TRIGGER 触发后自动复位，不累积计数。 */
    struct kevent wake_ev;
    EV_SET(&wake_ev, 0, EVFILT_USER, EV_ADD | EV_CLEAR, 0, 0, NULL);
    kevent(r->kq, &wake_ev, 1, NULL, 0, NULL);
    return r;
}

void rt_reactor_impl_destroy(void* backend) {
    RtReactorKqueue* r = (RtReactorKqueue*)backend;
    if (!r) return;
    if (r->kq >= 0) close(r->kq);
    free(r);
}

int32_t rt_reactor_impl_register(void* backend, int32_t fd, uint32_t events) {
    RtReactorKqueue* r = (RtReactorKqueue*)backend;
    if (!r || r->kq < 0) return -1;

    struct kevent changes[2];
    int nchanges = 0;

    if (events & RT_REACTOR_READABLE) {
        EV_SET(&changes[nchanges], fd, EVFILT_READ, EV_ADD | EV_ENABLE, 0, 0, NULL);
        nchanges++;
    }
    if (events & RT_REACTOR_WRITABLE) {
        EV_SET(&changes[nchanges], fd, EVFILT_WRITE, EV_ADD | EV_ENABLE, 0, 0, NULL);
        nchanges++;
    }

    if (nchanges > 0) {
        if (kevent(r->kq, changes, nchanges, NULL, 0, NULL) < 0) {
            return -1;
        }
    }
    return 0;
}

int32_t rt_reactor_impl_modify(void* backend, int32_t fd, uint32_t events) {
    /* modify = 重新 register（EV_ADD 是幂等的） */
    return rt_reactor_impl_register(backend, fd, events);
}

int32_t rt_reactor_impl_unregister(void* backend, int32_t fd) {
    RtReactorKqueue* r = (RtReactorKqueue*)backend;
    if (!r || r->kq < 0) return -1;

    struct kevent changes[2];
    EV_SET(&changes[0], fd, EVFILT_READ, EV_DELETE, 0, 0, NULL);
    EV_SET(&changes[1], fd, EVFILT_WRITE, EV_DELETE, 0, 0, NULL);
    /* EV_DELETE 对未注册的 filter 返回 ENOENT，忽略 */
    kevent(r->kq, changes, 2, NULL, 0, NULL);
    return 0;
}

/* kqueue 是就绪模型——submit_* 需要同步执行 IO（与 poll 类似） */

int32_t rt_reactor_impl_submit_read(void* backend, int32_t fd, void* buf,
                                     uint32_t len, uint64_t offset, void* user_data) {
    (void)backend; (void)offset; (void)user_data;
    ssize_t n = read(fd, buf, len);
    return (n < 0) ? -errno : (int32_t)n;
}

int32_t rt_reactor_impl_submit_write(void* backend, int32_t fd, const void* buf,
                                      uint32_t len, uint64_t offset, void* user_data) {
    (void)backend; (void)offset; (void)user_data;
    ssize_t n = write(fd, buf, len);
    return (n < 0) ? -errno : (int32_t)n;
}

int32_t rt_reactor_impl_submit_accept(void* backend, int32_t listen_fd, void* user_data) {
    (void)backend; (void)user_data;
    int32_t client_fd = (int32_t)accept(listen_fd, NULL, NULL);
    return (client_fd < 0) ? -errno : client_fd;
}

int32_t rt_reactor_impl_submit_connect(void* backend, int32_t fd,
                                        const void* addr, uint32_t addr_len, void* user_data) {
    (void)backend; (void)user_data;
    int32_t ret = connect(fd, (const struct sockaddr*)addr, addr_len);
    return (ret < 0) ? -errno : 0;
}

int32_t rt_reactor_impl_flush(void* backend) {
    (void)backend;
    return 0;
}

int32_t rt_reactor_impl_poll(void* backend, RtIoEvent* events, int32_t max_events,
                              int32_t timeout_ms) {
    RtReactorKqueue* r = (RtReactorKqueue*)backend;
    if (!r || r->kq < 0) return 0;

    struct kevent kevs[64];
    int32_t to_take = max_events < 64 ? max_events : 64;

    struct timespec ts;
    struct timespec* pts = NULL;
    if (timeout_ms >= 0) {
        ts.tv_sec = timeout_ms / 1000;
        ts.tv_nsec = (timeout_ms % 1000) * 1000000L;
        pts = &ts;
    }

    int n = kevent(r->kq, NULL, 0, kevs, to_take, pts);
    if (n < 0) {
        if (errno == EINTR) return 0;
        return -1;
    }

    int32_t result = 0;
    for (int i = 0; i < n && result < max_events; i++) {
        /* RFC 009 M6: 跳过 EVFILT_USER 唤醒哨兵（ident=0）——仅用于使
         * kevent 阻塞立即返回，不交付任何完成事件给上层 */
        if (kevs[i].filter == EVFILT_USER && kevs[i].ident == 0) {
            continue;
        }
        events[result].user_data = kevs[i].udata;
        events[result].fd = (int32_t)kevs[i].ident;
        events[result].result = 0;
        events[result].flags = 0;
        if (kevs[i].filter == EVFILT_READ) events[result].flags |= RT_REACTOR_READABLE;
        if (kevs[i].filter == EVFILT_WRITE) events[result].flags |= RT_REACTOR_WRITABLE;
        if (kevs[i].flags & EV_ERROR) events[result].flags |= RT_REACTOR_ERROR;
        if (kevs[i].flags & EV_EOF) events[result].flags |= RT_REACTOR_HANGUP;
        result++;
    }
    return result;
}

/* RFC 009 M6: 跨线程唤醒 —— 触发 EVFILT_USER 用户事件使阻塞的 kevent 返回。
 * 线程安全；EV_CLEAR 使触发后自动复位（无累积）。 */
void rt_reactor_impl_wake(void* backend) {
    RtReactorKqueue* r = (RtReactorKqueue*)backend;
    if (!r || r->kq < 0) return;
    struct kevent wake_ev;
    EV_SET(&wake_ev, 0, EVFILT_USER, 0, NOTE_TRIGGER, 0, NULL);
    kevent(r->kq, &wake_ev, 1, NULL, 0, NULL);
}

int32_t rt_reactor_impl_register_buffers(void* backend, const void** buffers,
                                          const uint32_t* lengths, int32_t n) {
    (void)backend; (void)buffers; (void)lengths; (void)n;
    return 0;  /* kqueue 无缓冲池注册 */
}

const char* rt_reactor_impl_backend_name(void) {
    return "kqueue";
}

void rt_reactor_impl_set_link_flag(void* backend, int32_t enable) {
    (void)backend; (void)enable; /* kqueue 不支持链式操作 */
}

int32_t rt_reactor_impl_submit_timeout(void* backend, uint64_t timeout_ns, void* user_data) {
    (void)backend; (void)timeout_ns; (void)user_data;
    return -1; /* kqueue 不支持 timeout 提交 */
}

#endif /* __APPLE__ || __FreeBSD__ */
