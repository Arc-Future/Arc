// EventLoop scheduler (RFC 009 M3 + M5.3 升级).
//
// 单线程 EventLoop 调度器：就绪队列（mutex 保护的 FIFO）+ 分级时间轮
// + waker 真实唤醒链路 + condition variable 避免 busy-wait。
//
// M5.3：M3 的有序单链表定时器升级为 3 级分级时间轮（rt_timer_wheel），
// 插入/到期均 O(1)，支撑高并发定时场景。
//
// 跨线程唤醒：外部线程调用 rt_waker_wake → g_rt_wake_fn → rt_event_loop_spawn
// → mutex lock → push ready queue → condvar signal → EventLoop 线程唤醒处理。
//
// Task.Delay(ms) 创建 Pending Task + 定时器；定时器到期时 tick
// 调用 rt_task_complete → Task 状态 READY + 触发 waker → 外层 Task 移入就绪队列。
//
// main entry wrapper 创建 EventLoop → spawn root task → run 直到完成。

#include "rt_abi.h"
#include <stdatomic.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>

/* ---- 丢失唤醒取证诊断计数器（临时：case2/case8 无探针时序挂死取证，定位后整体回收） ---- */
_Atomic(uint64_t) g_diag_wake_calls;
_Atomic(uint64_t) g_diag_wake_drop_el;
_Atomic(uint64_t) g_diag_wake_drop_data;
_Atomic(uint64_t) g_diag_coro_wake;
_Atomic(uint64_t) g_diag_poll_work;
_Atomic(uint64_t) g_diag_reg_late;
_Atomic(uint64_t) g_diag_add_prop;
/* el 线程心跳镜像（临时取证）：tick 计数 + pending/ready 快照，供 worker 侧
 * 转储判断 el 驱动线程是否停摆 */
_Atomic(uint64_t) g_diag_el_ticks;
_Atomic(uint32_t) g_diag_el_pending;
_Atomic(uint32_t) g_diag_el_ready;
/* el->mutex 持有者/等待者（定义于下方 mutex 区） */
_Atomic(long) g_diag_el_mutex_owner;
_Atomic(long) g_diag_el_mutex_waiters;
/* el 线程自身 tid（临时取证：跨线程取栈定位 tick 卡点） */
_Atomic(long) g_diag_el_tid;
/* el 循环相位（临时取证）：0=sleep 中 1=tick 2=fire_expired 3=exit-check 4=wait-entry */
_Atomic(uint32_t) g_diag_el_phase;
_Atomic(uint32_t) g_diag_el_has_reactor;

/* ---- 平台抽象 ---- */

/* el 阻塞等待的心跳上限（毫秒）：正常路径仍按定时器精确到期点阻塞；
 * 上限仅兜底丢失唤醒（timer 注册信号窗口、next_timeout=0 被译为无限
 * 等待等），与任务 park 的三态心跳同型——任何丢唤醒最多延迟一次空醒。
 * RFC 009 M6 的「无 100ms 轮询」契约指 IO/定时器延迟精度：signal/哨兵
 * 仍即时唤醒，本上限只约束最长静默期，不构成轮询。 */
#define RT_EL_HEARTBEAT_MS 50

#ifdef _WIN32
  #include <windows.h>
#else
  #include <pthread.h>
  #include <time.h>
#endif

/* ---- 平台抽象 ---- */

#ifdef _WIN32
  typedef CRITICAL_SECTION     rt_mutex_t;
  typedef CONDITION_VARIABLE   rt_cond_t;
#else
  #include <pthread.h>
  #include <time.h>
  typedef pthread_mutex_t      rt_mutex_t;
  typedef pthread_cond_t       rt_cond_t;
#endif

static void rt_el_mutex_init(rt_mutex_t* m) {
#ifdef _WIN32
    InitializeCriticalSection(m);
#else
    pthread_mutex_init(m, NULL);
#endif
}
static void rt_el_mutex_destroy(rt_mutex_t* m) {
#ifdef _WIN32
    DeleteCriticalSection(m);
#else
    pthread_mutex_destroy(m);
#endif
}
static void rt_el_mutex_lock(rt_mutex_t* m) {
#ifdef _WIN32
    if (!TryEnterCriticalSection(m)) {
        atomic_fetch_add_explicit(&g_diag_el_mutex_waiters, 1, memory_order_relaxed);
        EnterCriticalSection(m);
        atomic_fetch_sub_explicit(&g_diag_el_mutex_waiters, 1, memory_order_relaxed);
    }
#else
    pthread_mutex_lock(m);
#endif
    atomic_store_explicit(&g_diag_el_mutex_owner,
                          (long)GetCurrentThreadId(), memory_order_relaxed);
}
static void rt_el_mutex_unlock(rt_mutex_t* m) {
    atomic_store_explicit(&g_diag_el_mutex_owner, 0, memory_order_relaxed);
#ifdef _WIN32
    LeaveCriticalSection(m);
#else
    pthread_mutex_unlock(m);
#endif
}
static void rt_el_cond_init(rt_cond_t* c) {
#ifdef _WIN32
    InitializeConditionVariable(c);
#else
    pthread_cond_init(c, NULL);
#endif
}
static void rt_el_cond_signal(rt_cond_t* c) {
#ifdef _WIN32
    WakeConditionVariable(c);
#else
    pthread_cond_signal(c);
#endif
}
/* condvar wait（超时毫秒；0=立即返回不睡眠；调用方必须持 m）。
 * 不映射 INFINITE：next_timeout=0 表示时间轮已有到期定时器，若被翻译为
 * 无限等待，el 将永睡且到期 timer 永无 tick 可 fire → 丢失唤醒挂死
 * （v5 冻结现场：pending=1 + el_phase=4 + ticks 冻结 + timer 永不 fire）。
 * el 等待统一以 RT_EL_HEARTBEAT_MS 为上限（由 rt_el_wait_budget 钳制）。 */
static void rt_el_cond_wait(rt_cond_t* c, rt_mutex_t* m, uint64_t timeout_ms) {
#ifdef _WIN32
    if (timeout_ms == 0) return;
    SleepConditionVariableCS(c, m, (DWORD)timeout_ms);
#else
    if (timeout_ms == 0) return;
    struct timespec ts;
    clock_gettime(CLOCK_REALTIME, &ts);
    ts.tv_sec  += (time_t)(timeout_ms / 1000);
    ts.tv_nsec += (long)((timeout_ms % 1000) * 1000000);
    if (ts.tv_nsec >= 1000000000) { ts.tv_sec++; ts.tv_nsec -= 1000000000; }
    pthread_cond_timedwait(c, m, &ts);
#endif
}

/* 计算阻塞等待预算：0=已有到期定时器（立即返回交下一轮 tick fire）；
 * 短于心跳的保持精确值；达到/超过心跳（含「无定时器」哨兵 UINT64_MAX）
 * 一律钳制到 RT_EL_HEARTBEAT_MS——等待方永不无限睡。 */
static uint64_t rt_el_wait_budget(uint64_t next_timeout) {
    if (next_timeout == 0) return 0;
    if (next_timeout < RT_EL_HEARTBEAT_MS) return next_timeout;
    return RT_EL_HEARTBEAT_MS;
}

/* 单调时钟（毫秒） */
static uint64_t rt_now_ms(void) {
#ifdef _WIN32
    static LARGE_INTEGER freq = {0};
    if (freq.QuadPart == 0) {
        QueryPerformanceFrequency(&freq);
    }
    LARGE_INTEGER now;
    QueryPerformanceCounter(&now);
    return (uint64_t)(now.QuadPart * 1000 / freq.QuadPart);
#else
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (uint64_t)ts.tv_sec * 1000 + (uint64_t)ts.tv_nsec / 1000000;
#endif
}

/* ---- 定时器（M5.3：分级时间轮，委托 rt_timer_wheel） ---- */
/* rt_timer_node 定义已提升至 rt_abi.h（M5.3），此处直接使用。 */

/* ---- EventLoop ---- */

#define RT_READY_INIT_CAP 16

typedef struct RtEventLoop {
    /* 就绪队列（ring buffer） */
    void**    ready_queue;
    int32_t   ready_head;
    int32_t   ready_tail;
    int32_t   ready_capacity;
    int32_t   ready_count;

    /* M5.3：分级时间轮（替代 M3 有序链表） */
    rt_timer_wheel* timer_wheel;

    /* RFC 009 M2：Reactor（IO 后端，可选）
     * 通过 rt_event_loop_set_reactor 绑定；绑定时 tick 末尾调用 rt_reactor_poll
     * 处理就绪 IO 事件，run 用 reactor_poll 替代 condvar_wait 阻塞等待。 */
    void*    reactor;
    int32_t  has_reactor;

    /* 线程同步 */
    rt_mutex_t mutex;
    rt_cond_t  cond;

    /* RFC 009 M6：多线程 Executor —— 续体执行器（线程池）。
     * 由 rt_event_loop_set_threadpool 绑定（须在 run 前）；绑定后：
     *   - 就绪 Task 不再由 EventLoop 自 poll，改为投递 poll-task 工作项到线程池
     *   - g_rt_wake_fn 重定向为投递线程池（rt_task_threadpool_wake）
     * NULL=单线程回退（EventLoop 自 poll，原 M1-M5 行为）。 */
    rt_threadpool* threadpool;

    /* 状态 */
    int32_t   running;
    _Atomic int32_t pending_count;   /* 未完成 Task 数（root task + delay task）。
                                      * 原子化：wake/收口链（worker 持通道 Monitor
                                      * 经 SetResult→complete→wake 到达）不得阻塞于
                                      * el->mutex——AB-BA 环根因（与 el tick 内
                                      * continuation 取通道 Monitor 互为环）。 */
    /* 根任务指针：pending_count 仅对根任务递减（嵌套状态机子任务完成不得
     * 误减根任务计数 → 否则首轮嵌套 await 后 pending_count 归零、EventLoop
     * 提前退出，后续 await 永久挂起）。通过 rt_event_loop_set_root 设置。 */
    void*     root_task;
} RtEventLoop;

/* 全局 current EventLoop（单线程 MVP；跨线程唤醒通过此全局变量） */
static RtEventLoop* g_current_loop = NULL;

/* 全局 waker 回调函数指针（rt_task.c 通过此指针设置 Task 的 waker.wake）。
 * 在 rt_event_loop_create 时初始化为 rt_task_default_wake。 */
rt_waker_fn_ptr g_rt_wake_fn = NULL;

/* ---- waker 默认实现 ---- */

/* 默认 wake 回调：将 outer Task 移入就绪队列。
 * 可从任意线程调用（spawn 内部 mutex 保护）。 */
static void rt_task_default_wake(void* data) {
    RtEventLoop* loop = g_current_loop;
    if (!loop || !data) return;
    rt_event_loop_spawn(loop, data);
}

/* ---- RFC 009 M6：多线程 Executor（续体接入线程池） ---- */

/* poll-task 工作项：由线程池 worker 执行，推进 Task 状态机。
 * 生命周期契约：被 poll 的 Task 在 poll 期间必存活——async 语义保证 Task
 * 完成前其 await 链 owner 不释放它（owner 仅在 await 恢复并提取结果后才释放），
 * 与单线程 ready_queue 的持有语义一致，无需额外引用计数。
 * 多 worker 并发 poll 同一 Task 由 rt_task_poll 的 POLLING/NOTIFIED 守卫串行化。 */
/* pending_count 原子递减（0 下界保护，语义等价「若 >0 则减一」）。
 * 无锁：wake/收口链禁止阻塞于 el->mutex（AB-BA 破环，见字段注释）。 */
static void rt_el_pending_dec(RtEventLoop* el) {
    int32_t v = atomic_load_explicit(&el->pending_count, memory_order_relaxed);
    while (v > 0) {
        if (atomic_compare_exchange_weak_explicit(&el->pending_count, &v, v - 1,
                memory_order_relaxed, memory_order_relaxed)) {
            return;
        }
    }
}

static void rt_task_poll_work(void* arg) {
    RtTask* task = (RtTask*)arg;
    atomic_fetch_add_explicit(&g_diag_poll_work, 1, memory_order_relaxed);
    if (!task) return;
    int32_t st = rt_task_poll(task);
    /* 根任务完成 → 递减 pending_count 并唤醒 EventLoop 驱动线程检查退出。
     * 根任务（__async_main）仅启动时经 spawn 投递一次，完成后由本函数收口；
     * 嵌套子任务完成不得误减（root_task 指针精确匹配）。 */
    RtEventLoop* el = g_current_loop;
    if (el && task == el->root_task &&
        (st == RT_TASK_READY || st == RT_TASK_FAULTED)) {
        rt_el_pending_dec(el);
        /* 立即唤醒阻塞中的 EventLoop 驱动线程（Reactor 哨兵注入 / cond signal） */
        if (el->has_reactor && el->reactor) {
            rt_reactor_wake(el->reactor);
        } else {
            rt_el_cond_signal(&el->cond);
        }
    }
}

/* M6 wake 重定向：wake 时向线程池投递 poll-task（非 worker 线程 → 全局
 * injector，任意空闲 worker 拉取）。替代单线程 rt_task_default_wake。
 * 线程池未绑定（el->threadpool==NULL）时回退单线程 spawn。可从任意线程调用。 */
static void rt_task_threadpool_wake(void* data) {
    atomic_fetch_add_explicit(&g_diag_wake_calls, 1, memory_order_relaxed);
    RtEventLoop* el = g_current_loop;
    if (!el || !data) {
        if (!el) atomic_fetch_add_explicit(&g_diag_wake_drop_el, 1, memory_order_relaxed);
        else atomic_fetch_add_explicit(&g_diag_wake_drop_data, 1, memory_order_relaxed);
        return;
    }
    if (el->threadpool) {
        rt_work_t work;
        work.fn = rt_task_poll_work;
        work.data = data;
        rt_threadpool_spawn(el->threadpool, work);
    } else {
        rt_event_loop_spawn(el, data);
    }
}

/* RFC 009 M6：绑定线程池为续体执行器（多线程 executor）。
 * 绑定后 g_rt_wake_fn 重定向到线程池投递；传 NULL 解绑（回退单线程）。
 * 须在 rt_event_loop_run 前调用（驱动线程启动前完成绑定，无并发竞争）。 */
void rt_event_loop_set_threadpool(void* loop, void* pool) {
    RtEventLoop* el = (RtEventLoop*)loop;
    if (!el) return;
    rt_el_mutex_lock(&el->mutex);
    el->threadpool = (rt_threadpool*)pool;
    g_rt_wake_fn = el->threadpool ? rt_task_threadpool_wake : rt_task_default_wake;
    rt_el_mutex_unlock(&el->mutex);
}

/* ---- EventLoop ABI ---- */

void* rt_event_loop_create(void) {
    RtEventLoop* el = (RtEventLoop*)calloc(1, sizeof(RtEventLoop));
    if (!el) return NULL;
    el->ready_queue = (void**)calloc(RT_READY_INIT_CAP, sizeof(void*));
    if (!el->ready_queue) { free(el); return NULL; }
    el->ready_capacity = RT_READY_INIT_CAP;
    el->ready_head = 0;
    el->ready_tail = 0;
    el->ready_count = 0;
    el->timer_wheel = rt_timer_wheel_create();
    el->reactor = NULL;
    el->has_reactor = 0;
    el->running = 0;
    atomic_store_explicit(&el->pending_count, 0, memory_order_relaxed);
    el->root_task = NULL;
    rt_el_mutex_init(&el->mutex);
    rt_el_cond_init(&el->cond);
    /* 初始化全局 waker 回调（rt_task_register_waker 消费） */
    if (!g_rt_wake_fn) {
        g_rt_wake_fn = rt_task_default_wake;
    }
    return el;
}

void rt_event_loop_set_root(void* loop, void* task) {
    RtEventLoop* el = (RtEventLoop*)loop;
    if (!el) return;
    rt_el_mutex_lock(&el->mutex);
    el->root_task = task;
    rt_el_mutex_unlock(&el->mutex);
}

/* RFC 009 M2: 绑定 Reactor 作为 IO 后端。
 * 绑定后 tick 末尾调用 rt_reactor_poll 处理就绪 IO 事件，
 * run 用 reactor_poll 替代 condvar_wait 阻塞等待。
 * 重复调用覆盖前一次绑定；传 NULL 解绑。线程安全。 */
void rt_event_loop_set_reactor(void* loop, void* reactor) {
    RtEventLoop* el = (RtEventLoop*)loop;
    if (!el) return;
    rt_el_mutex_lock(&el->mutex);
    el->reactor = reactor;
    el->has_reactor = (reactor != NULL) ? 1 : 0;
    rt_el_mutex_unlock(&el->mutex);
}

/* RFC 009 M2: 查询当前绑定的 Reactor（无绑定返回 NULL）。 */
void* rt_event_loop_get_reactor(void* loop) {
    RtEventLoop* el = (RtEventLoop*)loop;
    if (!el) return NULL;
    return el->has_reactor ? el->reactor : NULL;
}

void rt_event_loop_destroy(void* loop) {
    RtEventLoop* el = (RtEventLoop*)loop;
    if (!el) return;
    /* H1: 勿 free ready_queue / timer_wheel / el——UnitTest async main 在
     * WriteReport 完整打印后 `main` 调 destroy；与残留 wake、默认池 Shutdown
     * 后 CRT 退出交织 → flaky 0xC0000005（应力 Summary 已出仍崩）。
     * 仅摘 g_current_loop；漏结构至进程退出。 */
    rt_el_mutex_lock(&el->mutex);
    el->running = 0;
    rt_el_mutex_unlock(&el->mutex);
    if (g_current_loop == el) g_current_loop = NULL;
}

void rt_event_loop_set_current(void* loop) {
    g_current_loop = (RtEventLoop*)loop;
}

void* rt_event_loop_current(void) {
    return g_current_loop;
}

/* 将 Task 加入就绪队列（线程安全）。可从任意线程调用。
 * RFC 009 M6：绑定线程池后，spawn 等价于向线程池投递 poll-task（续体由
 * worker 执行）；未绑定（单线程回退）时入 ready_queue 由 EventLoop 自 poll。 */
void rt_event_loop_spawn(void* loop, void* task) {
    RtEventLoop* el = (RtEventLoop*)loop;
    if (!el || !task) return;
    rt_el_mutex_lock(&el->mutex);
    if (el->threadpool) {
        rt_threadpool* pool = el->threadpool;
        rt_el_mutex_unlock(&el->mutex);
        rt_work_t work;
        work.fn = rt_task_poll_work;
        work.data = task;
        rt_threadpool_spawn(pool, work);
        return;
    }
    int32_t has_reactor = el->has_reactor;
    void* reactor = el->reactor;
    if (el->ready_count >= el->ready_capacity) {
        int32_t new_cap = el->ready_capacity * 2;
        void** new_queue = (void**)calloc((size_t)new_cap, sizeof(void*));
        if (!new_queue) {
            rt_el_mutex_unlock(&el->mutex);
            return;
        }
        for (int32_t i = 0; i < el->ready_count; i++) {
            int32_t idx = (el->ready_head + i) % el->ready_capacity;
            new_queue[i] = el->ready_queue[idx];
        }
        free(el->ready_queue);
        el->ready_queue = new_queue;
        el->ready_head = 0;
        el->ready_tail = el->ready_count;
        el->ready_capacity = new_cap;
    }
    el->ready_queue[el->ready_tail] = task;
    el->ready_tail = (el->ready_tail + 1) % el->ready_capacity;
    el->ready_count++;
    rt_el_cond_signal(&el->cond);
    rt_el_mutex_unlock(&el->mutex);
    /* RFC 009 M6：跨线程 spawn 唤醒阻塞中的驱动线程（Reactor 哨兵），
     * 使阻塞 poll 立即返回以处理新就绪 Task（单线程回退路径）。 */
    if (has_reactor && reactor) {
        rt_reactor_wake(reactor);
    }
}

/* 处理一轮就绪任务，返回处理数量 */
int32_t rt_event_loop_tick(void* loop) {
    RtEventLoop* el = (RtEventLoop*)loop;
    if (!el) return 0;
    int32_t processed = 0;
    rt_el_mutex_lock(&el->mutex);
    int32_t count = el->ready_count;
    void** snapshot = NULL;
    if (count > 0) {
        snapshot = (void**)malloc((size_t)count * sizeof(void*));
        if (snapshot) {
            for (int32_t i = 0; i < count; i++) {
                int32_t idx = (el->ready_head + i) % el->ready_capacity;
                snapshot[i] = el->ready_queue[idx];
            }
            el->ready_head = (el->ready_head + count) % el->ready_capacity;
            el->ready_count = 0;
            el->ready_tail = el->ready_head;
        }
    }
    rt_el_mutex_unlock(&el->mutex);

    if (!snapshot) return 0;

    /* RFC 009 M6：绑定线程池后，就绪 Task 投递到线程池执行（续体由 worker
     * 并发 poll，EventLoop 驱动线程不再自 poll）；未绑定（单线程回退）时
     * EventLoop 自 poll 并在此收口根任务 pending_count。 */
    rt_el_mutex_lock(&el->mutex);
    rt_threadpool* pool = el->threadpool;
    rt_el_mutex_unlock(&el->mutex);

    for (int32_t i = 0; i < count; i++) {
        void* task = snapshot[i];
        if (pool) {
            rt_work_t work;
            work.fn = rt_task_poll_work;
            work.data = task;
            rt_threadpool_spawn(pool, work);
        } else {
            int32_t status = rt_task_poll(task);
            /* pending_count 仅对根任务递减：嵌套状态机子任务完成若误减，会在此前
             * 根任务仍挂起于下一次 await 时把计数归零，导致 EventLoop 提前退出、
             * 后续嵌套 await 永久挂起（首轮嵌套 await 后 b2 不打印的根因）。
             * FAULTED 根任务同样终止：async 方法未捕获异常经 rt_task_poll SEH
             * 边界置 FAULTED，视为根任务完成（否则 pending_count 恒 1 → 挂死）。 */
            if ((status == RT_TASK_READY || status == RT_TASK_FAULTED)
                && task == el->root_task) {
                rt_el_pending_dec(el);
            }
        }
        processed++;
    }
    free(snapshot);

    /* RFC 009 M2: 处理就绪 IO 事件（非阻塞）。
     * reactor_poll 返回 N 个完成事件，每个事件的 user_data 是提交时传入的
     * RtIoCompletion 上下文（rt_socket_*_async 创建）。
     *
     * M2 完成：调用 rt_io_completion_complete 把 result 写回 Task（int_result/
     * ptr_result），然后 rt_task_complete 标记 READY + 触发 waker（将 outer
     * Task 移入就绪队列）。完成后 completion 上下文由函数内部释放。
     *
     * 向后兼容：若 user_data 不是 completion 上下文（旧路径纯 waker 指针），
     * rt_io_completion_complete 是 no-op，不会 crash。但 M2 后所有 async IO
     * 都走 completion 路径，waker 由 rt_task_complete 内部触发。 */
    if (el->has_reactor && el->reactor) {
        RtIoEvent io_events[64];
        int32_t n = rt_reactor_poll(el->reactor, io_events, 64, 0 /* 非阻塞 */);
        for (int32_t i = 0; i < n; i++) {
            void* user_data = io_events[i].user_data;
            if (user_data) {
                rt_io_completion_complete(user_data, io_events[i].result);
            }
            processed++;
        }
    }
    return processed;
}

/* 前置声明：rt_event_loop_pump 在 fire_expired 之前定义，需要 forward declaration。 */
static void rt_event_loop_fire_expired(RtEventLoop* el, uint64_t now_ms);

/* M4 mitigation：单轮驱动（tick + fire_expired），供 busy-wait await 路径调用。
 *
 * 背景：M2 状态机 lowering 条件限制导致 `if` 语句在 `await` 前触发 M1 同步
 * 回退。M1 busy-wait `rt_task_poll` 循环会让 EventLoop 永远得不到执行（死锁）。
 * 在 busy-wait 路径每次 poll 前调用 pump 推进定时器，让 Delay Task 得以就绪。
 *
 * 不阻塞：仅执行一轮 tick + fire_expired，立即返回。 */
void rt_event_loop_pump(void* loop) {
    RtEventLoop* el = (RtEventLoop*)loop;
    if (!el) return;
    rt_event_loop_tick(el);
    uint64_t now = rt_now_ms();
    rt_event_loop_fire_expired(el, now);
}

/* 添加定时器（内部函数，线程安全）。
 * M5.3：委托 rt_timer_wheel_add，O(1) 头插。
 * RFC 009 M6：新定时器可能比 run 当前阻塞等待的到期点更早 → 唤醒阻塞的
 * EventLoop 驱动线程。Reactor 绑定时经 rt_reactor_wake 注入哨兵使阻塞 poll
 * 立即返回（否则新定时器要等 poll 超时才被处理 → 「≤100ms 延迟」根因）；
 * 无 Reactor 时 cond_signal 即可（cond_wait 原子释放 mutex）。 */
static void rt_event_loop_add_timer_internal(RtEventLoop* el, rt_timer_node* timer) {
    rt_el_mutex_lock(&el->mutex);
    rt_timer_wheel_add(el->timer_wheel, timer);
    if (el->has_reactor && el->reactor) {
        void* reactor = el->reactor;
        rt_el_mutex_unlock(&el->mutex);
        rt_reactor_wake(reactor);
    } else {
        rt_el_cond_signal(&el->cond);
        rt_el_mutex_unlock(&el->mutex);
    }
}

/* 处理所有已到期的定时器。
 * M5.3：委托 rt_timer_wheel_tick，O(1) 均摊。 */
static void rt_event_loop_fire_expired(RtEventLoop* el, uint64_t now_ms) {
    if (!el) return;
    rt_el_mutex_lock(&el->mutex);
    rt_timer_wheel_tick(el->timer_wheel, now_ms);
    rt_el_mutex_unlock(&el->mutex);
}

/* 阻塞运行 EventLoop 直到 Stop 或无 pending task */
void rt_event_loop_run(void* loop) {
    RtEventLoop* el = (RtEventLoop*)loop;
    if (!el) return;
    el->running = 1;
    atomic_store_explicit(&g_diag_el_tid, (long)GetCurrentThreadId(),
                          memory_order_relaxed);
    atomic_store_explicit(&g_diag_el_has_reactor,
                          (uint32_t)(el->has_reactor && el->reactor),
                          memory_order_relaxed);
    while (el->running) {
        atomic_fetch_add_explicit(&g_diag_el_ticks, 1, memory_order_relaxed);
        atomic_store_explicit(&g_diag_el_pending,
                              (uint32_t)atomic_load_explicit(&el->pending_count,
                                                             memory_order_relaxed),
                              memory_order_relaxed);
        atomic_store_explicit(&g_diag_el_ready, (uint32_t)el->ready_count,
                              memory_order_relaxed);
        /* 1. 处理就绪队列 + RFC 009 M2: Reactor IO 事件（tick 末尾非阻塞 poll） */
        atomic_store_explicit(&g_diag_el_phase, 1, memory_order_relaxed);
        rt_event_loop_tick(el);
        /* 2. 处理定时器 */
        atomic_store_explicit(&g_diag_el_phase, 2, memory_order_relaxed);
        uint64_t now = rt_now_ms();
        rt_event_loop_fire_expired(el, now);
        /* 3. 判断是否应该退出 */
        atomic_store_explicit(&g_diag_el_phase, 3, memory_order_relaxed);
        rt_el_mutex_lock(&el->mutex);
        int32_t has_pending = atomic_load_explicit(&el->pending_count,
                                                   memory_order_relaxed);
        int32_t has_ready = el->ready_count;
        /* M5.3：next_timeout 由时间轮扫描得出；UINT64_MAX 表示无定时器 */
        uint64_t next_timeout = rt_timer_wheel_next_timeout(el->timer_wheel);
        int32_t has_timer = (next_timeout != UINT64_MAX);
        if (has_pending == 0 && has_ready == 0 && !has_timer) {
            rt_el_mutex_unlock(&el->mutex);
            break;
        }
        /* 4. 无就绪 task 时，阻塞等待 IO 事件或定时器到期。
         *
         * 阻塞时长计算：
         *   - 有定时器：wait_ms = next_timeout（到下一定时器到期）
         *   - 无定时器：wait_ms = -1（无限等待，直到 IO 事件 / 哨兵唤醒）
         *
         * RFC 009 M6 跨线程唤醒（多线程 Executor 精确性）：
         *   - **阻塞等待期间不持 el->mutex**——若持锁阻塞，worker 线程添加定时器
         *     （rt_task_delay）会被 mutex 卡住直至 poll 超时 → 「每次 Task.Delay 实际
         *     耗时 ~100ms 的延迟杀手」根因。cond_wait 原子释放/重获 mutex，故无锁阻塞；
         *     reactor_poll 则须先显式释放。
         *   - 跨线程信号（新定时器 / 跨线程 spawn / 根任务完成 / stop）经
         *     rt_reactor_wake 注入哨兵（IOCP PostQueuedCompletionStatus /
         *     kqueue EVFILT_USER），使阻塞 poll 立即返回——下一轮迭代重算
         *     next_timeout 按精确到期点阻塞，**无 100ms 轮询兜底**。 */
        if (has_ready == 0) {
            if (el->has_reactor && el->reactor) {
                void* reactor = el->reactor;
                /* 等待预算同型钳制（-1 无限 poll 是哨兵丢失挂死形态——
                 * el 阻塞等待一律 ≤ RT_EL_HEARTBEAT_MS，见 rt_el_wait_budget）。 */
                int32_t timeout_ms = (next_timeout == UINT64_MAX || next_timeout > 0x7FFFFFFF)
                    ? (int32_t)RT_EL_HEARTBEAT_MS
                    : (int32_t)rt_el_wait_budget(next_timeout);
                rt_el_mutex_unlock(&el->mutex); /* 不持锁阻塞 poll */
                atomic_store_explicit(&g_diag_el_phase, 4, memory_order_relaxed);
                /* 阻塞 poll 返回的完成事件必须**立即处理**，不得留给下一轮 tick 的
                 * 非阻塞 poll——IOCP 的 GetQueuedCompletionStatusEx 会从完成端口
                 * **出队**事件，若在此丢弃，下一轮非阻塞 poll 将取不到任何事件，
                 * IO Task 永不完成 → 真异步读（HTTP/SSE 响应头）永久挂起。故对返回
                 * 事件即刻调用 rt_io_completion_complete（写回 Task + 触发 waker），
                 * 下一轮 tick 处理就绪的 outer Task。 */
                RtIoEvent io_events[16];
                int32_t n = rt_reactor_poll(reactor, io_events, 16, timeout_ms);
                for (int32_t i = 0; i < n; i++) {
                    void* user_data = io_events[i].user_data;
                    if (user_data) {
                        rt_io_completion_complete(user_data, io_events[i].result);
                    }
                }
            } else {
                /* 无 Reactor 模式：condvar_wait（原 RFC 009 M3 行为）。
                 * cond_wait 原子释放 mutex 阻塞；超时/信号唤醒后重获 mutex。
                 * 等待预算经 rt_el_wait_budget 钳制：永不无限睡（心跳兜底）。 */
                uint64_t wait_ms = rt_el_wait_budget(next_timeout);
                atomic_store_explicit(&g_diag_el_phase, 4, memory_order_relaxed);
                rt_el_cond_wait(&el->cond, &el->mutex, wait_ms);
                rt_el_mutex_unlock(&el->mutex);
            }
        } else {
            rt_el_mutex_unlock(&el->mutex);
        }
    }
    el->running = 0;
}

void rt_event_loop_stop(void* loop) {
    RtEventLoop* el = (RtEventLoop*)loop;
    if (!el) return;
    rt_el_mutex_lock(&el->mutex);
    el->running = 0;
    int32_t has_reactor = el->has_reactor;
    void* reactor = el->reactor;
    rt_el_cond_signal(&el->cond);
    rt_el_mutex_unlock(&el->mutex);
    /* RFC 009 M6：Reactor 绑定时注入哨兵使阻塞 poll 立即返回，stop 即时生效 */
    if (has_reactor && reactor) {
        rt_reactor_wake(reactor);
    }
}

void rt_event_loop_inc_pending(void* loop) {
    RtEventLoop* el = (RtEventLoop*)loop;
    if (!el) return;
    /* 无锁：本函数在 wake 链上（可能持有用户 Monitor），阻塞于 el->mutex
     * 即成 AB-BA 环；signal 锁外发出，丢失窗口由 50ms 等待预算兜底。 */
    atomic_fetch_add_explicit(&el->pending_count, 1, memory_order_relaxed);
    rt_el_cond_signal(&el->cond);
}

void rt_event_loop_dec_pending(void* loop) {
    RtEventLoop* el = (RtEventLoop*)loop;
    if (!el) return;
    rt_el_pending_dec(el);
    rt_el_cond_signal(&el->cond);
}

/* ---- Task.Delay ABI ---- */

/* 创建 Delay Task：Pending 状态 + 定时器。定时器到期后 Task 变 READY + 触发 waker。
 * 需在 EventLoop 线程调用（使用 g_current_loop）。 */
void* rt_task_delay(int32_t milliseconds) {
    if (milliseconds < 0) milliseconds = 0;
    /* slab 分配（自动注册活任务表 + 复用）：裸 calloc 任务链外身份会让其首次
     * 合法释放误触 [DOUBLE-UNREG]（diag_linked 恒 0），且普查漏计 Delay 任务。 */
    RtTask* t = (RtTask*)rt_task_slab_alloc();
    if (!t) return NULL;
    t->status = RT_TASK_PENDING;
    t->canceled = 0;
    t->resume = NULL;
    t->resume_data = NULL;
    t->waker = NULL;

    rt_timer_node* timer = (rt_timer_node*)calloc(1, sizeof(rt_timer_node));
    if (!timer) { rt_task_slab_free(t); return NULL; }
    timer->deadline_ms = rt_now_ms() + (uint64_t)milliseconds;
    /* M4 通用化：fn=rt_task_complete, data=task（等价 M3 的 timer->task=t 行为） */
    timer->fn = rt_task_complete;
    timer->data = t;
    timer->canceled = 0;
    timer->next = NULL;

    if (g_current_loop) {
        rt_event_loop_add_timer_internal(g_current_loop, timer);
        /* M4 修复：不再 inc_pending。pending_count 仅跟踪 root task（由 main wrapper inc）。
         * Delay task 的完成通过时间轮 count + waker 链路驱动：
         *   - timer 在 wheel count 中计数，EventLoop 不会提前退出
         *   - timer 到期 → rt_task_complete → waker → spawn outer → outer 在 tick 中 dec pending
         * 此前 inc_pending 导致 Delay 的 +1 永不 dec（Delay 不进 ready queue），
         * 使 WhenAll/WhenAny 等场景 EventLoop 永不退出。 */
    } else {
        /* 无 EventLoop：立即完成（fallback 到同步路径） */
        t->status = RT_TASK_READY;
        free(timer);
    }
    return t;
}

/* RFC 009 M4: 通用定时器回调注册。
 * 创建 rt_timer_node + 注册到 EventLoop 时间轮。
 * delay_ms 后调用 fn(data)。用于 CTS.CancelAfter + 未来 IO 异步等。
 * 需在 EventLoop 线程调用（使用传入的 loop）。 */
void rt_event_loop_schedule(void* loop, void(*fn)(void*), void* data, uint64_t delay_ms) {
    RtEventLoop* el = (RtEventLoop*)loop;
    if (!el || !fn) return;
    rt_timer_node* timer = (rt_timer_node*)calloc(1, sizeof(rt_timer_node));
    if (!timer) return;
    timer->deadline_ms = rt_now_ms() + delay_ms;
    timer->fn = fn;
    timer->data = data;
    timer->canceled = 0;
    timer->next = NULL;
    rt_event_loop_add_timer_internal(el, timer);
}

/* RFC 009 M4: Task.Delay(ms, ct) 取消传播。
 * ct 取消回调：标记 Delay Task 为 CANCELED + 触发 waker。
 * Delay Task 已通过 rt_task_delay 注册定时器；ct 取消时定时器可能尚未到期，
 * 此时直接 cancel + complete 让 outer 尽快被唤醒（status 保持 CANCELED 因 canceled=1）。 */
static void rt_task_delay_ct_callback(void* data) {
    RtTask* delay_task = (RtTask*)data;
    if (!delay_task) return;
    rt_task_cancel(delay_task);       /* canceled=1, status=CANCELED */
    rt_task_complete(delay_task);     /* 触发 waker（status 保持 CANCELED） */
}

/* Task.Delay(ms, ct) → 创建 Pending Delay Task + 定时器 + 注册 ct 回调。
 * ct 已取消时立即取消 Delay Task；否则注册回调，ct 取消时触发 cancel+complete。
 * ct=NULL 时等价于 rt_task_delay(ms)。 */
void* rt_task_delay_ct(int32_t milliseconds, void* ct) {
    void* delay_task = rt_task_delay(milliseconds);
    if (!delay_task || !ct) return delay_task;
    /* ct 已取消：立即取消 Delay Task */
    if (rt_cts_is_canceled(ct)) {
        rt_task_cancel(delay_task);
        rt_task_complete(delay_task);
        return delay_task;
    }
    /* 注册 ct 取消回调：ct 取消时标记 Delay Task 为 CANCELED + 触发 waker */
    rt_cts_register(ct, rt_task_delay_ct_callback, delay_task);
    return delay_task;
}
