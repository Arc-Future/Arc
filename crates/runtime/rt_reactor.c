// Reactor 统一入口 + 后端分发（RFC 009 M1）。
//
// 跨平台 IO 多路复用抽象，后端可插拔：
//   - Linux: io_uring（批量提交 + 零拷贝，主推）
//   - Windows: IOCP（IO Completion Port）
//   - macOS/FreeBSD: kqueue
//   - 嵌入式回退: poll
//
// 编译期选择后端（RFC 009 §4.2）：
//   rt_reactor.c 通过 #include 平台后端实现文件，所有 ABI 入口
//   委托给平台后端的 rt_reactor_impl_* 函数。
//
// 设计原则（RFC 009 §0.4）：
//   1. API 表面不变性——所有平台暴露相同 ABI
//   2. 分层降级——无 io_uring 降级 epoll/poll，功能不丢
//   3. 零分配热路径——Reactor 内部环形缓冲预分配
//   4. 批量化——N 个 IO 操作从 N syscalls 降为 1 syscall

#include "rt_abi.h"
#include <stdlib.h>
#include <string.h>

/* ============================================================
 * 平台后端选择
 *
 * 每个后端实现以下内部接口（rt_reactor_impl_*）：
 *   void* rt_reactor_impl_create(uint32_t flags);
 *   void  rt_reactor_impl_destroy(void* backend);
 *   int32_t rt_reactor_impl_register(void* backend, int32_t fd, uint32_t events);
 *   int32_t rt_reactor_impl_modify(void* backend, int32_t fd, uint32_t events);
 *   int32_t rt_reactor_impl_unregister(void* backend, int32_t fd);
 *   int32_t rt_reactor_impl_submit_read(void* backend, int32_t fd, void* buf,
//                                      uint32_t len, uint64_t offset, void* user_data);
//   ... (write/accept/connect 同形)
//   int32_t rt_reactor_impl_submit_timeout(void* backend, uint64_t timeout_ns, void* user_data);
//   void    rt_reactor_impl_set_link_flag(void* backend, int32_t enable);
//   int32_t rt_reactor_impl_flush(void* backend);
//   int32_t rt_reactor_impl_poll(void* backend, RtIoEvent* events,
//                               int32_t max_events, int32_t timeout_ms);
//   int32_t rt_reactor_impl_register_buffers(void* backend, const void** buffers,
//                                           const uint32_t* lengths, int32_t n);
//   const char* rt_reactor_impl_backend_name(void);
 * ============================================================ */

#if defined(__linux__)
  /* Linux: io_uring 主推（raw syscall，不依赖 liburing） */
  #include "platform/reactor_io_uring.c"
  #define RT_REACTOR_HAS_BACKEND 1
#elif defined(_WIN32) || defined(_WIN64)
  /* Windows: IOCP */
  #include "platform/reactor_iocp.c"
  #define RT_REACTOR_HAS_BACKEND 1
#elif defined(__APPLE__) || defined(__FreeBSD__)
  /* macOS / FreeBSD: kqueue */
  #include "platform/reactor_kqueue.c"
  #define RT_REACTOR_HAS_BACKEND 1
#else
  /* 嵌入式 / 未知平台: poll 回退 */
  #include "platform/reactor_poll.c"
  #define RT_REACTOR_HAS_BACKEND 1
#endif

/* ============================================================
 * 统一 ABI 入口 —— 委托给平台后端
 *
 * Reactor 句柄直接持有 backend 指针，无需额外封装层。
 * ============================================================ */

void* rt_reactor_create(void) {
    return rt_reactor_impl_create(0);
}

void* rt_reactor_create_sqpoll(void) {
    return rt_reactor_impl_create(RT_REACTOR_FLAG_SQPOLL);
}

void rt_reactor_destroy(void* reactor) {
    if (!reactor) return;
    rt_reactor_impl_destroy(reactor);
}

int32_t rt_reactor_register(void* reactor, int32_t fd, uint32_t events) {
    if (!reactor) return -1;
    return rt_reactor_impl_register(reactor, fd, events);
}

int32_t rt_reactor_modify(void* reactor, int32_t fd, uint32_t events) {
    if (!reactor) return -1;
    return rt_reactor_impl_modify(reactor, fd, events);
}

int32_t rt_reactor_unregister(void* reactor, int32_t fd) {
    if (!reactor) return -1;
    return rt_reactor_impl_unregister(reactor, fd);
}

int32_t rt_reactor_submit_read(void* reactor, int32_t fd, void* buf,
                                uint32_t len, uint64_t offset, void* user_data) {
    if (!reactor) return -1;
    return rt_reactor_impl_submit_read(reactor, fd, buf, len, offset, user_data);
}

int32_t rt_reactor_submit_write(void* reactor, int32_t fd, const void* buf,
                                 uint32_t len, uint64_t offset, void* user_data) {
    if (!reactor) return -1;
    return rt_reactor_impl_submit_write(reactor, fd, buf, len, offset, user_data);
}

int32_t rt_reactor_submit_accept(void* reactor, int32_t listen_fd, void* user_data) {
    if (!reactor) return -1;
    return rt_reactor_impl_submit_accept(reactor, listen_fd, user_data);
}

int32_t rt_reactor_submit_connect(void* reactor, int32_t fd,
                                   const void* addr, uint32_t addr_len, void* user_data) {
    if (!reactor) return -1;
    return rt_reactor_impl_submit_connect(reactor, fd, addr, addr_len, user_data);
}

int32_t rt_reactor_submit_timeout(void* reactor, uint64_t timeout_ns, void* user_data) {
    if (!reactor) return -1;
    return rt_reactor_impl_submit_timeout(reactor, timeout_ns, user_data);
}

void rt_reactor_set_link_flag(void* reactor, int32_t enable) {
    if (!reactor) return;
    rt_reactor_impl_set_link_flag(reactor, enable);
}

int32_t rt_reactor_flush(void* reactor) {
    if (!reactor) return -1;
    return rt_reactor_impl_flush(reactor);
}

int32_t rt_reactor_poll(void* reactor, RtIoEvent* events, int32_t max_events,
                         int32_t timeout_ms) {
    if (!reactor || !events || max_events <= 0) return 0;
    return rt_reactor_impl_poll(reactor, events, max_events, timeout_ms);
}

/* RFC 009 M6: 跨线程唤醒 —— 注入哨兵使阻塞中的 rt_reactor_poll 立即返回。
 * 由多线程 executor 的 worker 线程在根任务完成时调用，唤醒 EventLoop 驱动
 * 线程及时检查退出（消除「≤100ms 轮询兜底」延迟）。各后端实现 rt_reactor_impl_wake。 */
void rt_reactor_wake(void* reactor) {
    if (!reactor) return;
    rt_reactor_impl_wake(reactor);
}

int32_t rt_reactor_register_buffers(void* reactor, const void** buffers,
                                     const uint32_t* lengths, int32_t n) {
    if (!reactor) return -1;
    return rt_reactor_impl_register_buffers(reactor, buffers, lengths, n);
}

const char* rt_reactor_backend_name(void* reactor) {
    (void)reactor;
    return rt_reactor_impl_backend_name();
}
