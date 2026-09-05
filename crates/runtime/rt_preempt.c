// 异步抢占支持（RFC 009 M2）。
//
// 平台抽象：
//   - Linux: SIGURG 信号（Go 1.14 方案），signal handler 设置 preempt_requested 标志
//   - Windows: QueueUserAPC（APC 注入），worker 在 SleepConditionVariableCS 可中断等待时执行
//   - 降级：无信号支持时退化为协作式（M2 检测平台支持，自动降级）
//
// 抢占语义：
//   1. 定时器轮每 1ms tick 检查 worker 执行时间
//   2. 若 worker 执行同一 task > 1ms，发送 SIGURG（Linux）/ QueueUserAPC（Windows）
//   3. signal handler / APC 回调设置 worker->preempt.preempt_requested = 1
//   4. worker 在下一个 await 点（codegen 生成）检查 preempt_requested
//   5. 若为 1：当前 task 不推回队列（由调用方 rt_task_poll 判断）→ 返回 PENDING
//      让 worker 重新调度，清除 preempt_requested
//
// 关键约束：
//   - signal handler 不分配内存、不获取锁（异步信号安全）
//   - 抢占仅在 await 点生效（协作式，非强制中断）
//   - 纯 CPU-bound 紧凑循环（无 await）无法抢占 → 仍需 Task.Run 隔离

#include "rt_abi.h"
#include <stdatomic.h>
#include <stdlib.h>
#include <string.h>

#ifdef _WIN32
  #ifndef WIN32_LEAN_AND_MEAN
    #define WIN32_LEAN_AND_MEAN
  #endif
  #include <windows.h>
#else
  #include <signal.h>
  #include <pthread.h>
  #include <time.h>
  #include <unistd.h>
#endif

/* ---- 平台实现：Windows QueueUserAPC ---- */

#if defined(_WIN32)

static void NTAPI rt_apc_callback(ULONG_PTR param) {
    if (param) {
        atomic_store_explicit((_Atomic(int32_t)*)(ULONG_PTR)param, 1,
                              memory_order_release);
    }
}

void rt_preempt_init(void) {
    /* Windows APC 机制无需初始化 */
}

static int rt_preempt_supported(void) {
    return 1;  /* Windows 始终支持 APC */
}

void rt_preempt_signal_impl(_Atomic(int32_t)* preempt_requested) {
    if (!preempt_requested) return;
    HANDLE thread = GetCurrentThread();
    QueueUserAPC(rt_apc_callback, thread, (ULONG_PTR)preempt_requested);
}

/* ---- 平台实现：Linux SIGURG ---- */

/* 仅 __linux__ 启用：SIGURG 在 macOS/BSD 亦定义但无 sigqueue(2)，
 * 误入本分支会留下潜伏链接错误（平台审计 S2 #5）——此类平台走下方
 * 协作式降级（rt_preempt_is_supported()=0，语义与文档「自动降级」一致）。 */
#elif defined(SIGURG) && defined(__linux__)

/* 平台支持标志：1=支持 SIGURG，0=降级协作式 */
static int32_t g_preempt_supported = 1;

/* signal handler 不访问 rt_preempt_state（避免在 signal context 中追踪 worker），
 * 改为通过 siginfo si_value 直接传递 preempt_requested flag 指针。 */
static void rt_sigurg_handler(int sig, siginfo_t* info, void* ctx) {
    (void)sig;
    (void)ctx;
    if (info && info->si_value.sival_ptr) {
        atomic_store_explicit(
            (_Atomic(int32_t)*)info->si_value.sival_ptr,
            1,
            memory_order_release);
    }
}

void rt_preempt_init(void) {
    struct sigaction sa;
    memset(&sa, 0, sizeof(sa));
    sa.sa_sigaction = rt_sigurg_handler;
    sa.sa_flags = SA_SIGINFO | SA_RESTART;
    sigemptyset(&sa.sa_mask);
    sigaddset(&sa.sa_mask, SIGURG);
    if (sigaction(SIGURG, &sa, NULL) != 0) {
        g_preempt_supported = 0;  /* 降级：无抢占 */
    }
}

static int rt_preempt_supported(void) {
    return g_preempt_supported;
}

void rt_preempt_signal_impl(_Atomic(int32_t)* preempt_requested) {
    if (!g_preempt_supported || !preempt_requested) return;
    union sigval sv;
    sv.sival_ptr = (void*)preempt_requested;
    sigqueue(getpid(), SIGURG, sv);
}

/* ---- 降级路径：无信号支持平台 ---- */

#else

void rt_preempt_init(void) {
    /* 降级：无抢占 */
}

static int rt_preempt_supported(void) {
    return 0;  /* 降级：协作式 */
}

void rt_preempt_signal_impl(_Atomic(int32_t)* preempt_requested) {
    (void)preempt_requested;
    /* no-op on unsupported platforms */
}

#endif

/* ---- 公共 ABI ---- */

int32_t rt_preempt_check(rt_preempt_state* s) {
    if (!s) return 0;
    /* 读取 preempt_requested；若已设置，返回 1 让调用方处理。
     * 不在此处清除 —— 由 rt_preempt_clear 专门清除，避免 signal handler
     * 与 await 边界并发竞态（signal 在 check 后 set → 标志未消费即被清）。 */
    int32_t requested = atomic_load_explicit(&s->preempt_requested,
                                              memory_order_acquire);
    return requested;
}

int32_t rt_preempt_was_triggered(rt_preempt_state* s) {
    if (!s) return 0;
    /* 与 rt_preempt_check 分离：check 在 await 边界主动检查，
     * was_triggered 在 rt_task_poll 外层查询是否因抢占触发 PENDING。
     * 两个 API 分离避免 check 误清标志导致 rt_task_poll 无法感知抢占。 */
    int32_t requested = atomic_load_explicit(&s->preempt_requested,
                                              memory_order_acquire);
    return requested;
}

void rt_preempt_clear(rt_preempt_state* s) {
    if (!s) return;
    atomic_store_explicit(&s->preempt_requested, 0, memory_order_release);
}

void rt_preempt_record_start(rt_preempt_state* s, int64_t now_ms) {
    if (!s) return;
    atomic_store_explicit(&s->exec_start_ms, now_ms, memory_order_release);
}

int32_t rt_preempt_is_supported(void) {
    return rt_preempt_supported();
}

/* ---- worker ctx 便捷封装（codegen 调用，避免 GEP 偏移硬编码） ---- */

int32_t rt_worker_preempt_check(rt_worker_ctx* w) {
    if (!w) return 0;
    return rt_preempt_check(&w->preempt);
}

void rt_worker_preempt_clear(rt_worker_ctx* w) {
    if (!w) return;
    rt_preempt_clear(&w->preempt);
}
