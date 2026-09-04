// Async task ABI (RFC 015 Phase A + RFC 009 M1/M2/M3).
//
// Phase A 占位：rt_task_poll 内联调用 resume，无真实调度。
// RFC 009 M1 扩展：泛型结果提取（ptr/value）、取消标记、状态查询、
//                 状态机 env 句柄（M2 消费）、waker 注册（M3 消费）。
// RFC 009 M2 升级：resume 签名改为 `int32_t (*)(void* env, rt_waker* waker)`，
//                 返回新 status（READY/PENDING/CANCELED/FAULTED）。
//                 rt_task_poll 用返回值更新 Task 状态。
// RFC 009 M3 扩展：rt_task_complete（完成通知+触发 waker）、
//                 rt_task_register_waker（默认 waker 注册）。
//
// EventLoop 调度器由 rt_event_loop.c 实现。

#include "rt_abi.h"
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#ifdef _WIN32
#include <intrin.h> /* _ReturnAddress（rt_task_poll NULL 契约取证） */
#endif

/* 丢失唤醒取证诊断计数器（临时：定义于 rt_event_loop.c，定位后整体回收） */
extern _Atomic(uint64_t) g_diag_coro_wake;
extern _Atomic(uint64_t) g_diag_reg_late;
extern _Atomic(uint64_t) g_diag_add_prop;

#if defined(_WIN32)
#  define WIN32_LEAN_AND_MEAN
#  define NOMINMAX
#  include <windows.h> /* SEH: EXCEPTION_EXECUTE_HANDLER / EXCEPTION_CONTINUE_SEARCH */
#endif

#if defined(_WIN32)
/* rt_task_poll 的 SEH 边界过滤器（自包含，不依赖 rt_exc.c）：
 * 捕获 rt_throw → _CxxThrowException 抛出的 MSVC C++ 异常（0xE06D7363），
 * 从 ExceptionInformation[1] 解出 Arc 异常对象载荷 {void* obj}；
 * 其余原生异常返回 0（CONTINUE_SEARCH，由系统处理）。 */
__declspec(thread) static void* rt_task_poll_exc = NULL;
static int rt_task_exc_capture(int code, PEXCEPTION_POINTERS ep, void** out) {
    if (code != 0xE06D7363 || !ep || !ep->ExceptionRecord) return 0;
    if (ep->ExceptionRecord->NumberParameters < 2) return 0;
    void* payload = (void*)ep->ExceptionRecord->ExceptionInformation[1];
    if (!payload) return 0;
    *out = *(void**)payload;
    return 1;
}
#endif

/* waker 交接自旋锁（定义于 rt_waker_wake 之后；声明见 rt_abi.h，
 * 此处前置声明供 poll_inner / set_waker 等早期使用点调用） */
void rt_task_wk_lock(RtTask* t);
void rt_task_wk_unlock(RtTask* t);
/* 唤醒链事件级 trace（定义于 wk_lock 区；complete/poll_inner 前向引用） */
static void rt_wk_trace(char kind, void* inner, void* outer, int32_t st);
/* 取证总开关（定义于 wk_trace 区；release 等早期热路径点前向引用） */
static int rt_diag_enabled(void);

/* poll 相位追踪（临时取证）：0=无 1=已获POLLING 2=resume中 3=resume返回
 * 4=完成临界区 5=re-poll 循环迭代 */
static _Atomic(uint32_t) g_diag_poll_phase;
static RtTask* g_diag_poll_task;
static _Atomic(uint32_t) g_diag_reg_in_flight;
static RtTask* g_diag_reg_inner;
/* re-poll 活锁取证：poll 进入数 / re-poll 迭代数（rt_threadpool 转储读取） */
_Atomic(uint64_t) g_diag_poll_entries;
_Atomic(uint64_t) g_diag_repoll_iters;
/* complete 时无 waker（await 未注册即完成）计数：应≈reglate（补发闭合），
 * 若显著大于 reglate → 补发/注册链路丢唤醒实锤。 */
_Atomic(uint64_t) g_diag_complete_no_waker;
void rt_diag_btrace(const char* tag); /* 定义于 rt_threadpool.c */
void* rt_mon_diag_current_owner_obj_of(long tid); /* 定义于 rt_thread.c */

/* POLLING 持有者审计表（临时取证）：CAS 成功登记 {task, tid}，释放清除。
 * 转储列出全部未清表项——泄漏者（线程与任务）无所遁形。 */
typedef struct rt_poll_own_slot {
    RtTask* task;
    long    tid;
} rt_poll_own_slot;
#define RT_POLL_OWN_SLOTS 16
static rt_poll_own_slot g_poll_owners[RT_POLL_OWN_SLOTS];
static _Atomic(uint32_t) g_poll_owners_lock;

void rt_diag_poll_own(RtTask* t, int acquire) {
    uint32_t expected = 0u;
    while (!atomic_compare_exchange_strong_explicit(
            &g_poll_owners_lock, &expected, 1u,
            memory_order_acquire, memory_order_relaxed)) {
        expected = 0u;
    }
    if (acquire) {
#ifdef _WIN32
        long tid = (long)GetCurrentThreadId();
#else
        long tid = 0;
#endif
        for (int i = 0; i < RT_POLL_OWN_SLOTS; i++) {
            if (g_poll_owners[i].task == NULL) {
                g_poll_owners[i].task = t;
                g_poll_owners[i].tid = tid;
                break;
            }
        }
    } else {
#ifdef _WIN32
        long tid = (long)GetCurrentThreadId();
#else
        long tid = 0;
#endif
        for (int i = 0; i < RT_POLL_OWN_SLOTS; i++) {
            if (g_poll_owners[i].task == t && g_poll_owners[i].tid == tid) {
                g_poll_owners[i].task = NULL;
                g_poll_owners[i].tid = 0;
                break;
            }
        }
    }
    atomic_store_explicit(&g_poll_owners_lock, 0u, memory_order_release);
}

void rt_diag_poll_own_dump(void) {
    for (int i = 0; i < RT_POLL_OWN_SLOTS; i++) {
        if (g_poll_owners[i].task != NULL) {
            fprintf(stderr, "[poll-owner] t=%p tid=%ld\n",
                    (void*)g_poll_owners[i].task, g_poll_owners[i].tid);
        }
    }
    /* 嵌套审计：持 POLLING 且同时持 Monitor 的线程 = 「CS 内嵌套 poll」形态。
     * 若该 poll 的任务永不完成且 Monitor 不释放 → CS 内嵌套死锁环实锤。 */
    for (int i = 0; i < RT_POLL_OWN_SLOTS; i++) {
        if (g_poll_owners[i].task != NULL) {
            void* mon = rt_mon_diag_current_owner_obj_of(g_poll_owners[i].tid);
            if (mon) {
                fprintf(stderr, "[nested-poll] t=%p tid=%ld mon_obj=%p\n",
                        (void*)g_poll_owners[i].task, g_poll_owners[i].tid, mon);
            }
        }
    }
}

/* acquire 环形栈缓冲（临时取证）：每次 POLLING acquire 捕获 12 帧栈。
 * 泄漏时交叉 poll-owner 表 → 每个「仍持有」任务的 acquire 点栈直接可见。 */
#define RT_DIAG_RING 32
#define RT_DIAG_FRAMES 12
static _Atomic(uint64_t) g_diag_ring_pos;
typedef struct rt_diag_ring_slot {
    RtTask* task;
    long    tid;
    void*   frames[RT_DIAG_FRAMES];
} rt_diag_ring_slot;
static rt_diag_ring_slot g_diag_ring[RT_DIAG_RING];

static void rt_diag_ring_record(RtTask* t) {
#ifdef _WIN32
    long tid = (long)GetCurrentThreadId();
#else
    long tid = 0;
#endif
    uint64_t pos = atomic_fetch_add_explicit(&g_diag_ring_pos, 1,
                                             memory_order_relaxed);
    rt_diag_ring_slot* s = &g_diag_ring[pos % RT_DIAG_RING];
    s->task = t;
    s->tid = tid;
#ifdef _WIN32
    CaptureStackBackTrace(0, RT_DIAG_FRAMES, s->frames, NULL);
#else
    for (int i = 0; i < RT_DIAG_FRAMES; i++) s->frames[i] = NULL;
#endif
}

void rt_diag_ring_dump(void) {
    uint64_t pos = atomic_load_explicit(&g_diag_ring_pos, memory_order_relaxed);
    for (int k = 0; k < RT_DIAG_RING; k++) {
        uint64_t idx = (pos - 1 - (uint64_t)k) % RT_DIAG_RING;
        rt_diag_ring_slot* s = &g_diag_ring[idx];
        if (!s->task) continue;
        fprintf(stderr, "[acq] t=%p tid=%ld frames:", (void*)s->task, s->tid);
        for (int f = 0; f < RT_DIAG_FRAMES; f++) {
            void* fr = s->frames[f];
            if (!fr) continue;
#ifdef _WIN32
            /* 打印模块相对 RVA（ASLR 稳定，llvm-symbolizer --obj 离线符号化） */
            HMODULE mod = NULL;
            if (GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
                                   (LPCWSTR)fr, &mod) && mod) {
                char mod_path[MAX_PATH];
                if (GetModuleFileNameA(mod, mod_path, MAX_PATH) > 0) {
                    const char* base = mod_path;
                    for (const char* p = mod_path; *p; p++) {
                        if (*p == '\\' || *p == '/') base = p + 1;
                    }
                    unsigned long long rva =
                        (unsigned long long)((char*)fr - (char*)mod);
                    fprintf(stderr, " %s+0x%llx", base, rva);
                    continue;
                }
            }
#endif
            fprintf(stderr, " %p", fr);
        }
        fprintf(stderr, "\n");
    }
}

/* 已移除 P0 诊断守卫（地址环形表 UAF/DF 检测）：该守卫按地址记录释放，
 * 后续分配复用同一地址时产生假阳性 abort（preempt_await_path 中
 * pool.Run task 释放后 Task.Delay 复用其地址），故删除，仅保留原子修复。 */

/* RtTask 和 rt_resume_fn 定义已在 rt_abi.h 中（M3 共享给 rt_event_loop.c） */

/* RFC 006 子项 M1：Task.FromResult 值缓存哨兵。必须在首次使用（rt_task_release）
 * 之前定义，否则 C 预处理器按顺序展开会报 undeclared identifier。 */
#define RT_TASK_FROM_CACHE (-1)

/* RFC 008：follower 级联扇出（定义见本文件 TCS 支撑段；release 提前使用）。 */
static void rt_task_fire_followers(RtTask* t);

/* ---- 已有（Phase A） ---- */

/* M5.2：优先使用 per-worker slab 分配；slab 不可用时回退 calloc。
 * slab 路径在 worker 线程热路径零 malloc（free_list pop），性能 ~5ns vs ~80ns。
 * 非工作线程（如未初始化 slab 的主线程）保持 calloc 路径以兼容现有行为。 */
RtTask* rt_task_alloc(void) {
    return rt_task_slab_alloc();
}

/* M5.2：Task 释放统一入口。
 * 按 from_slab 标志分流：slab 来源→free_list push；malloc 来源→free。
 * 调用方需确保 Task 已完成且不再被引用（无 waker 待触发、无 result 待读取）。
 * 同时释放 resume_data（状态机 env，由 codegen emit_sm_ctor 中 calloc 分配）。
 *
 * RFC 009 M3：若 Task 有 dtor_fn，优先调用它释放 env（处理 spilled 指针 + free env）；
 * 否则直接 free(resume_data)（M2 路径）。这是通用机制——runtime 不感知 spill 语义。 */
void rt_task_release(void* state) {
    RtTask* t = (RtTask*)state;
    if (!t) return;
    /* RFC 006 子项 M1：Task.FromResult 值缓存（对标 .NET 单例缓存）。
     * 缓存单例由 rt_init_result_cache 以 from_slab = RT_TASK_FROM_CACHE 哨标记，
     * 进程生命周期内常驻、不可回收。此处直接返回，不走 value_result/resume_data/
     * slab_free 路径——避免共享单例被回收（复用 waker 槽入 free_list）或重复释放。
     * 非缓存 Task 的 from_slab ∈ {0,1}（slab=1 / calloc=0），恒不触此分支。 */
    if (t->from_slab == RT_TASK_FROM_CACHE) return;
    /* 释放对账（取证，ARC_DIAG=1 开启）：resume==NULL 即 Delay/外部事件任务
     *（rt_task_delay），协程/状态机任务 resume=状态机函数指针。同一 t 出现
     * 两次 [REL] 即 double-release；caller（返回地址 RVA）直接指认两方调用点。
     * 限 20000 行。热路径默认关闭（CaptureStackBackTrace 开销大）。 */
    if (rt_diag_enabled()) {
        static _Atomic(int32_t) rel_log_count;
        if (atomic_fetch_add_explicit(&rel_log_count, 1,
                                      memory_order_relaxed) < 20000) {
            char caller_rva[40] = "?";
#ifdef _WIN32
            void* caller = NULL;
            if (CaptureStackBackTrace(1, 1, &caller, NULL) > 0) {
                HMODULE mod = NULL;
                char mp[MAX_PATH];
                if (GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS |
                                       GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                                       (LPCWSTR)caller, &mod) && mod &&
                    GetModuleFileNameA(mod, mp, MAX_PATH) > 0) {
                    const char* b = mp;
                    for (const char* p = mp; *p; p++) {
                        if (*p == '\\' || *p == '/') b = p + 1;
                    }
                    _snprintf_s(caller_rva, sizeof(caller_rva), _TRUNCATE,
                                "%s+0x%llx", b,
                                (unsigned long long)((char*)caller - (char*)mod));
                }
            }
#endif
            fprintf(stderr, "[REL] t=%p resume=%p st=%d tid=%lu caller=%s\n",
                    (void*)t, (void*)t->resume,
                    atomic_load_explicit((_Atomic int32_t*)&t->status,
                                         memory_order_relaxed),
                    (unsigned long)GetCurrentThreadId(), caller_rva);
        }
    }
    /* RFC 016 子项 M2：FAULTED Task 的异常所有权统一转移。
     * 异常对象恒为 Arc class（rt_task_fault 转移 throw 在途 +1 / rt_task_from_exception
     * 发射处 inc），Task 持唯一引用；此处 dec 归还——配合 await 提取侧「先 inc 再 release」
     * 保证提取后残留恰为 await 副本一份，达成零泄漏、零 DUP。
     * RFC 009 §结果所有权（强持有，2026-08-22 收敛）：非 FAULTED 的 class 结果
     * （ptr_is_class=1）同样由 Task 强持有 +1，此处统一 dec 归还；string/array/
     * Task/Func（ptr_is_class=0，无 ArcHeader）为借引用，不在此 dec（RFC 009 §
     * 结果所有权「任何路径都不 retain」）。 */
    if (t->ptr_result && t->ptr_is_class) {
        rt_arc_dec(t->ptr_result);
        t->ptr_result = NULL;
        t->ptr_is_class = 0;
    }
    /* RFC 008：丢弃未完成的 TCS leader（仍挂 follower）→ 级联取消 follower，
     * 防 await 挂死（对标 tokio oneshot：Sender drop 唤醒 Receiver）。 */
    if (t->follower_head) {
        t->canceled = 1;
        rt_task_fire_followers(t);
    }
    /* 释放 value_result 的 malloc'd copy（若存在） */
    if (t->value_result) {
        free(t->value_result);
        t->value_result = NULL;
    }
    /* RFC 009 M3：优先调用 dtor_fn 释放 env（处理 spilled 指针），否则直接 free */
    if (t->resume_data) {
        if (t->dtor_fn) {
            t->dtor_fn(t->resume_data);  /* dtor 内部 free(env) */
        } else {
            free(t->resume_data);  /* M2 路径：无 spill，直接 free */
        }
        t->resume_data = NULL;
    }
    rt_task_slab_free(t);
}

/* ---- RFC 006 子项 M1：Task.FromResult 值缓存（对标 .NET Task.FromResult 单例）----
 *
 * .NET 自 6.0 起对 bool（true/false 两个单例）与热 int 值的完成 Task 使用进程级
 * 单例缓存，避免每次 FromResult 重复分配 Task。Arc 对齐：对 int 族
 * （int/bool/byte/char/short/uint/ushort/sbyte——codegen 在 emit_call.rs L3496 统一
 * 收敛拓宽到 i32 后调 rt_task_from_int）的 [0,255] 值缓存进程级单例 Task，覆盖
 * bool 全域、byte 全域与热 int 值；[0,255] 之外回退普通 slab 分配。
 *
 * 共享安全论证：
 *   - READY 单例行不可变（status=READY、resume=NULL、int_result 恒为缓存值），
 *     await 提取 / Wait / ContinueWith 对 READY Task 均只读（poll 非 PENDING 即
 *     resume 直通，不注册 waker；continue_with 对 READY 走同步直调）→ 零污染。
 *   - release/cancel 以 from_slab=RT_TASK_FROM_CACHE 哨兵守卫，单例不可回收、
 *     不可取消（对标 .NET 对已完成 Task 抛 IAE）。
 *   - 跨类型共享同一 RtTask 句柄运行期安全：Task<T> 句柄不透明，int_result 值
 *     一致，类型安全由编译期保证（如 Task.FromResult(true) 与 Task.FromResult(1)
 *     共享同一单例，但类型上不可互赋）。
 *
 * 生命周期：进程常驻，不进入 slab free_list（与 .NET GC 托管单例同效，24KB 级
 * 有界保留）。急切初始化经 __attribute__((constructor))（沿用 rt_file.c L1221
 * 先例）；若构造器未运行（极端链接），数组全 NULL，rt_task_from_int 自然回退
 * 普通分配，优雅降级。 */
#define RT_TASK_CACHE_MIN 0
#define RT_TASK_CACHE_MAX 255
static RtTask* g_result_cache[RT_TASK_CACHE_MAX + 1];

static void rt_init_result_cache(void) {
    for (int i = RT_TASK_CACHE_MIN; i <= RT_TASK_CACHE_MAX; i++) {
        RtTask* t = (RtTask*)calloc(1, sizeof(RtTask));
        if (!t) continue;
        t->status = RT_TASK_READY;
        t->int_result = i;
        t->from_slab = RT_TASK_FROM_CACHE;
        g_result_cache[i] = t;
    }
}
__attribute__((constructor)) static void rt_result_cache_ctor(void) {
    rt_init_result_cache();
}

void* rt_task_from_int(int32_t value) {
    if (value >= RT_TASK_CACHE_MIN && value <= RT_TASK_CACHE_MAX) {
        RtTask* cached = g_result_cache[value];
        if (cached) return cached;
    }
    RtTask* t = rt_task_alloc();
    if (t) t->int_result = value;
    return t;
}

void* rt_task_void(void) {
    return rt_task_alloc();
}

/* M6: 无守卫 poll 核心（单次推进状态机）。rt_task_poll 的 POLLING/NOTIFIED
 * 守卫内部调用；多线程下同一 Task 的并发 poll 由外层守卫串行化（防 resume 重入）。 */
static int32_t rt_task_poll_inner(RtTask* t) {
    /* RFC 038 下钻：poll 是观察点。生产者可能先 spawn 一批任务（滞留 TLS 批，
     * 未达 RT_TP_BATCH 未自动发布），随后 poll 观察完成——若不冲刷，任务永不
     * 注入、worker 空转，poll 恒 PENDING → 死循环（wait_all_isolated path A 复现）。
     * 故 poll 前必须冲刷本线程生产者批，保证「已 spawn 必已发布」。 */
    rt_threadpool_flush_local();
    // 已取消的任务不再推进
    if (t->canceled) return RT_TASK_CANCELED;
    /* P0 teardown race 修复（2026-08-03 二次）：status 读取用 acquire，
     * 与 rt_task_complete 的 release store 配对——观察者一旦读到 READY，
     * 即同步于 complete 侧所有先前 store（含 t->waker = NULL），
     * 随后 rt_task_release 的 free 不会与 worker 残余写并发。 */
    int32_t st = atomic_load_explicit((_Atomic(int32_t)*)&t->status,
                                      memory_order_acquire);
    // M2: resume 返回新 status（READY/PENDING/FAULTED）。
    // Phase A 同步 Task（resume=NULL）直接返回当前 status。
    if (st == RT_TASK_PENDING && t->resume) {
#if defined(_WIN32)
        /* 状态机 resume 抛出的 Arc 异常（await 提取点 rethrow / 函数体 throw）
         * 在 EventLoop/线程池等纯 C 驱动上下文无 C++ catch 边界——原实现直穿
         * 终止进程（WER 0xC0000409 subcode 7 = terminate→abort）。此处镜像
         * rt_task_run_trampoline 的异常边界语义：捕获 → rt_task_fault 置
         * FAULTED 并存储异常，交由 await/Wait 侧 rt_task_is_faulted → rt_throw
         * rethrow。与 trampoline 不同，过滤不依赖 TLS rt_exception（自包含于
         * rt_task.c，避免对 rt_exc.c 的链接依赖）：Arc 异常经 rt_throw →
         * _CxxThrowException 以 MSVC C++ 异常（0xE06D7363）原生 raise，
         * ExceptionInformation[1] 指向载荷 {void* obj}（布局经 probe 实证，
         * 与 rt_exc.c 手工 ThrowInfo 同源）。原生异常（访问违例等）一律
         * EXCEPTION_CONTINUE_SEARCH，绝不吞噬。 */
        __try {
            atomic_store_explicit(&g_diag_poll_phase, 2, memory_order_relaxed);
            st = t->resume(t->resume_data, t->waker);
        } __except (rt_task_exc_capture(GetExceptionCode(), GetExceptionInformation(),
                                        &rt_task_poll_exc)
                        ? EXCEPTION_EXECUTE_HANDLER
                        : EXCEPTION_CONTINUE_SEARCH) {
            rt_task_fault(t, rt_task_poll_exc);
            st = RT_TASK_FAULTED;
        }
#else
        atomic_store_explicit(&g_diag_poll_phase, 2, memory_order_relaxed);
        st = t->resume(t->resume_data, t->waker);
#endif
        atomic_store_explicit((_Atomic(int32_t)*)&t->status, st,
                              memory_order_relaxed);
        /* M6.2 暖启动守卫 Case B：resume 完成路径（st != PENDING）清除残留守卫位。
         * 「wake 先于置位」竞态（wake 在 poll(inner) 与 register_waker 之间清位、
         * register_waker 后置位 → 残留 1）下，NOTIFIED 重 poll 同步续跑至完成时
         * 必须清位——否则 READY Task 因残留位被误判 PENDING → 父 await 挂死。
         * 位恒 0 时幂等零开销。 */
        if (st != RT_TASK_PENDING) {
            /* 完成路径与 rt_task_complete 同策（wk_lock 临界区）：status 终态
             * store（release）与 waker snapshot 对 register_waker 的 install +
             * status 复检互斥——否则「awaiter 的 register 与本完成并发」交错：
             * 本处裸读 waker 为 NULL（尚未安装）→ 不触发；register 的复检读到
             * 旧 PENDING（原 relaxed store 无同步）→ 安装在已终态任务上 →
             * 零唤醒源永挂（WhenAll 聚合任务经本路径完成，case8 高频复现）。 */
            rt_task_wk_lock(t);
            atomic_store_explicit((_Atomic(int32_t)*)&t->status, st,
                                  memory_order_release);
            rt_waker* wk = t->waker;
            t->waker = NULL;
            rt_task_wk_unlock(t);
            rt_wk_trace('P', t, wk != NULL ? wk->data : NULL, st);
            atomic_store_explicit((_Atomic(uint32_t)*)&t->await_waiting, 0u,
                                  memory_order_release);
            /* M5.5 fix: 状态机通过 resume 路径完成时（status 离开 PENDING）触发 waker，
             * 唤醒等待此 Task 的 outer Task。M3 之前 rt_task_complete 是唯一 waker
             * 触发点（Delay/IO 等外部事件），但 state machine 经 resume 完成时不会
             * 调用 rt_task_complete，导致 outer Task 永远不被唤醒——
             * delay_with_return_value / cross_await_local_survival 测试失败根因。
             * 前置条件：waker 已通过 rt_task_register_waker 注册（outer 在 await 时设置）。
             * 快照后锁外触发；wk_lock 保证与 register/complete 互斥，至多单次 fire。 */
            if (wk) {
                rt_waker_wake(wk);
            }
        }
        /* RFC 009 M2：抢占触发的 PENDING → 推回 LIFO slot 等待下次调度。
         * rt_task_poll 在 EventLoop 上下文调用时无法获取 ThreadPool worker_ctx；
         * EventLoop 的抢占语义不同（单线程调度器，抢占意义有限）。
         * 真正的抢占推回由 ThreadPool worker 主循环处理——worker 执行 work.fn
         * 后检查 was_triggered，若触发则重新 spawn_local 到 LIFO slot。
         * rt_task_poll 本身只传递状态，不直接操作 LIFO slot。 */
    }
    return st;
}

int32_t rt_task_poll(void* state) {
    RtTask* t = (RtTask*)state;
    if (!t) {
        /* D3 契约收敛（RFC 050 §5 / stability 评审）：NULL poll 返 READY 曾把
         * 「坏唤醒/状态机腐坏后的 NULL Task」静默降级为「已完成 + 空结果」——
         * 错误以 default 值继续流转，爆炸点远离根因。改 fail-fast：取证现场
         * （返回地址）后响亮终止，错误在发生点暴露。运行时全部调用方已核
         * 具 NULL 预防（rt_combinator 遍历跳空 / rt_task_poll_work 预检）。 */
        void* ret = _ReturnAddress();
        fprintf(stderr, "[poll-null] rt_task_poll(NULL) — contract violation, ret=%p\n", ret);
        fflush(stderr);
        abort();
    }
    /* M6.2 warm-start 守卫顶部门控：在 CAS 抢占 POLLING 之前，先读 await_waiting。
     *
     * 背景：暖启动（async 调用点 autostart 首 poll）后本 Task 以 PENDING 暴露，
     * 父 await 的「直达提取」若越界 resume 未完成 inner 即重入违例。守卫置位
     * （await 挂起时 register_waker 设 1）表示「以外层唤醒为准，禁止异地 poll 推进」。
     *
     * 规则（只读位，不在此清位）：
     *   - 见位非 0 → 返 PENDING。位仅由两处清 0：
     *       a) rt_task_coro_wake（waker 触发，恒与「inner 已完成」配对）——清位后
     *          才投递 outer，此后外层 poll 越过守卫（恒 waker 驱动，闭合竞态）；
     *       b) rt_task_poll_inner 完成路径（st != PENDING）——清除「wake 先于置位」
     *          竞态残留位（Case B：wake 在 poll(inner) 与 register_waker 之间清位，
     *          register_waker 后置位 → 残留 1；NOTIFIED 重 poll 同步续跑至完成时
     *          必须清位，否则 READY Task 因残留位被误判 PENDING → 父 await 挂死）。
     *   - 位恒 0（同步 Task / 无 await 未置位）→ 零开销直下。 */
    if (atomic_load_explicit((_Atomic(uint32_t)*)&t->await_waiting,
                             memory_order_acquire) != 0u) {
        return RT_TASK_PENDING;
    }
    /* M6 多线程守卫（多线程 executor）：CAS 抢占 POLLING 位。
     * 成功 → 本线程持有 poll 权，进入 inner 推进状态机；
     * 失败 → 其他 worker 正在 poll 该 Task → 置 NOTIFIED 位（请求持有者
     *       释放时重 poll）并返回 PENDING。
     *
     * 「poll 中唤醒」闭合协议（case2/case8 第三处丢失唤醒根治，2026-09-03）：
     * 旧实现存在通知吞噬窗口——失败方置 NOTIFIED 后即返回，全凭持有者在
     * fetch_and(~POLLING) 时读到该位；若 fetch_or 恰落在持有者的 fetch_and
     * 之后，持有者读到的旧 flags 不含 NOTIFIED → 不重 poll，而投递方已返回、
     * inner 已终态、waker 已被 complete snapshot 清空 → outer 挂起后零唤醒源
     * （channels backpressure 高频复现：check-after-register 补发 poll-task
     * 恰落于此窗口）。修复为双向验证闭环：
     *   - 失败方：置 NOTIFIED 后**复验** poll_flags——POLLING 仍在 → 持有者
     *     必经 fetch_and 读到本位（同位置修改序），安全返回；POLLING 已释放
     *     → 持有者错过本请求 → 失败方自己重试 CAS（不得依赖已被吞噬的请求）；
     *   - 持有方：释放时读到 NOTIFIED → 消费该位并**重新持锁**（CAS）重 poll。
     *     旧实现裸调 poll_inner 无 POLLING 保护，重 poll 期间与并发 poll-task
     *     重入——重 poll 期间新到的 NOTIFIED 由新一轮释放检查承接，闭环无洞。
     * 重 poll 迭代不重入顶部 await_waiting 守卫：触发 NOTIFIED 的 wake 必已
     * 清位（coro_wake 先清位再投递），且重 poll 的语义正是观察「register 前
     * 已完成」的 inner（Case B 残留位由 poll_inner 完成路径清除）。 */
    int32_t st = RT_TASK_PENDING;
    atomic_fetch_add_explicit(&g_diag_poll_entries, 1, memory_order_relaxed);
    for (;;) {
        uint32_t expected = 0u;
        if (atomic_compare_exchange_strong_explicit(
                (_Atomic(uint32_t)*)&t->poll_flags, &expected, RT_TASK_PF_POLLING,
                memory_order_acq_rel, memory_order_relaxed)) {
            atomic_store_explicit(&g_diag_poll_phase, 1, memory_order_relaxed);
            g_diag_poll_task = t;
            rt_diag_poll_own(t, 1); /* 取证：登记 POLLING 持有者 */
            rt_diag_ring_record(t); /* 取证：acquire 栈入环 */
            st = rt_task_poll_inner(t);
            atomic_store_explicit(&g_diag_poll_phase, 3, memory_order_relaxed);
            /* 释放 POLLING；持有期间被置 NOTIFIED（并发 poll-task / 补发竞争）
             * → 消费该位并重新持锁重 poll（见上：双向验证闭环）。 */
            uint32_t flags = atomic_fetch_and_explicit(
                (_Atomic(uint32_t)*)&t->poll_flags, ~RT_TASK_PF_POLLING,
                memory_order_release);
            rt_diag_poll_own(t, 0); /* 取证：清除持有者登记 */
            if (flags & RT_TASK_PF_NOTIFIED) {
                atomic_fetch_and_explicit((_Atomic(uint32_t)*)&t->poll_flags,
                                          ~RT_TASK_PF_NOTIFIED, memory_order_relaxed);
                atomic_store_explicit(&g_diag_poll_phase, 5, memory_order_relaxed);
                atomic_fetch_add_explicit(&g_diag_repoll_iters, 1, memory_order_relaxed);
                continue;
            }
            atomic_store_explicit(&g_diag_poll_phase, 0, memory_order_relaxed);
            g_diag_poll_task = NULL;
            return st;
        }
        /* 持有竞争：请求持有者重 poll，然后复验请求是否被覆盖 */
        atomic_fetch_or_explicit((_Atomic(uint32_t)*)&t->poll_flags,
                                 RT_TASK_PF_NOTIFIED, memory_order_relaxed);
        uint32_t seen = atomic_load_explicit((_Atomic(uint32_t)*)&t->poll_flags,
                                             memory_order_relaxed);
        if (seen & RT_TASK_PF_POLLING) {
            /* 持有者尚未释放：其 fetch_and 在修改序上后于本 fetch_or，必读到
             * NOTIFIED 并重 poll——本请求已被承接，安全返回 */
            return RT_TASK_PENDING;
        }
        /* 持有者已释放且其释放读先于本 fetch_or → 它已错过本请求，重试。
         * 先清掉本请求的 NOTIFIED：重试 CAS 以 expected=0 抢占，NOTIFIED 残留
         * 会使 CAS 永远失败 → 烧 CPU 自旋（无持有者时该位本无读者——所有
         * 失败 poller 各自走本 validate 路径重试，互不依赖此位）。 */
        atomic_fetch_and_explicit((_Atomic(uint32_t)*)&t->poll_flags,
                                  ~RT_TASK_PF_NOTIFIED, memory_order_relaxed);
    }
}

int32_t rt_task_result_int(void* state) {
    RtTask* t = (RtTask*)state;
    return t ? t->int_result : 0;
}

/* ---- RFC 009 M1 新增 ---- */

void* rt_task_from_ptr(void* value) {
    RtTask* t = rt_task_alloc();
    if (t) t->ptr_result = value;
    return t;
}

/* RFC 009 §结果所有权（强持有，2026-08-22 收敛）：class FromResult 专用。
 * 调用方（codegen）须先 rt_arc_inc 授予 task +1 再传入（借用源）；fresh
 * 值的 +1 直接移交。置 ptr_is_class=1 使 release 统一 dec。string/array
 * （无 ArcHeader，immortal 借用）走 rt_task_from_ptr 不置位。 */
void* rt_task_from_class(void* value) {
    RtTask* t = rt_task_alloc();
    if (t) {
        t->ptr_result = value;
        t->ptr_is_class = 1;
    }
    return t;
}

void* rt_task_from_value(void* data, int32_t size) {
    RtTask* t = rt_task_alloc();
    if (!t) return NULL;
    if (!data || size <= 0) return t;
    t->value_result = malloc((size_t)size);
    if (!t->value_result) {
        t->status = RT_TASK_FAULTED;
        return t;
    }
    memcpy(t->value_result, data, (size_t)size);
    t->value_size = size;
    return t;
}

int32_t rt_task_status(void* state) {
    RtTask* t = (RtTask*)state;
    if (!t) return RT_TASK_READY;
    if (t->canceled) return RT_TASK_CANCELED;
    /* P0 修复：acquire 读与 complete 的 release store 配对。 */
    return atomic_load_explicit((_Atomic(int32_t)*)&t->status,
                                memory_order_acquire);
}

void* rt_task_result_ptr(void* state) {
    RtTask* t = (RtTask*)state;
    return t ? t->ptr_result : NULL;
}

void rt_task_result_value(void* state, void* dst, int32_t size) {
    RtTask* t = (RtTask*)state;
    if (t && t->value_result && dst && size > 0 && size <= t->value_size) {
        memcpy(dst, t->value_result, (size_t)size);
    }
}

void rt_task_cancel(void* state) {
    RtTask* t = (RtTask*)state;
    if (!t) return;
    /* RFC 006 M1：缓存单例为已完成任务，不可取消（对标 .NET 对已完成 Task 抛
     * InvalidOperationException）。取消会写入 canceled/status，污染被共享的单例。 */
    if (t->from_slab == RT_TASK_FROM_CACHE) return;
    t->canceled = 1;
    t->status = RT_TASK_CANCELED;
}

int32_t rt_task_is_canceled(void* state) {
    RtTask* t = (RtTask*)state;
    return t ? t->canceled : 0;
}

void* rt_task_from_state_machine(void* env, void* resume_fn) {
    RtTask* t = rt_task_alloc();
    if (t) {
        t->status = RT_TASK_PENDING;
        t->resume_data = env;
        t->resume = (rt_resume_fn)resume_fn;
        t->dtor_fn = NULL;  /* RFC 009 M3：默认无 dtor，codegen 有 spill 时通过 rt_task_set_dtor_fn 覆盖 */
    }
    return t;
}

/* I2：协程 Task ABI（CoroSplit 单帧所有权）。codegen 直线体 async 走 LLVM
 * 协程路径时以单次调用创建 Task——运行时直接持帧所有权：resume_data=帧、
 * resume=thunk（桥接 rt_resume_fn 契约）、dtor_fn=destroy thunk（其体仅
 * coro.destroy(%frame)）。rt_task_release 经 dtor_fn 调 destroy 释放帧。
 * 与通用 rt_task_from_state_machine 解耦（去除两步间接），使 coro 路径不复
 * 依赖旧状态机路径（plan.md 阶段 3 I3 删 rt_task_from_state_machine 前置）。 */
void* rt_task_from_coroutine(void* frame, void* resume_fn, void (*destroy_fn)(void* frame)) {
    RtTask* t = rt_task_alloc();
    if (t) {
        t->status = RT_TASK_PENDING;
        t->resume_data = frame;
        t->resume = (rt_resume_fn)resume_fn;
        t->dtor_fn = destroy_fn;  /* 帧销毁交 coro.destroy thunk */
    }
    return t;
}

/* ---- RFC 008 AsyncStream：TaskCompletionSource 支撑 ----
 *
 * rt_task_create_pending：分配 PENDING 态 Task（rt_task_alloc 清零即 READY，
 * 无状态机 env/resume——完成由外部事件驱动，经 rt_task_complete/adopt 收口）。
 * 供 std TaskCompletionSource<T> 构造未完成句柄。 */
void* rt_task_create_pending(void) {
    RtTask* t = rt_task_alloc();
    if (t) {
        t->status = RT_TASK_PENDING;
        t->follower_head = NULL;
        t->follower_next = NULL;
    }
    return t;
}

/* RFC 008：单 follower 结果传播（级联扇出 / add_follower 已完成同步共用）。
 * 结果字段语义（RFC 009 §结果所有权 强持有，2026-08-22 收敛）：
 *   - int（READY）：直接拷贝。
 *   - ptr（READY）：leader 与 follower 共享只读视图——class 结果（ptr_is_class=1）
 *     每 follower 各持 +1（此处 inc 授予、release 对等 dec），string/array/Task/Func
 *     为借引用直接拷贝不 inc（无 ArcHeader，任何路径不 retain）。
 *   - value_result：深拷贝 malloc 块——follower release 时 free 自己的副本，
 *     leader 的块仍归 leader。
 *   - FAULTED 异常：每 follower 独立在途 +1（fault 内不 inc，此处授予），
 *     follower release / await 提取时 dec 归还。 */
static void rt_task_propagate(RtTask* l, RtTask* f) {
    f->int_result = l->int_result;
    f->ptr_result = l->ptr_result;
    f->ptr_is_class = l->ptr_is_class;
    if (f->ptr_result && f->ptr_is_class) {
        rt_arc_inc(f->ptr_result);
    }
    if (l->value_result && l->value_size > 0) {
        void* buf = malloc((size_t)l->value_size);
        if (buf) {
            memcpy(buf, l->value_result, (size_t)l->value_size);
            f->value_result = buf;
            f->value_size = l->value_size;
        }
    }
    if (l->canceled) {
        f->canceled = 1;
        f->status = RT_TASK_CANCELED;
    }
    if (l->status == RT_TASK_FAULTED && f->ptr_result) {
        rt_arc_inc(f->ptr_result);
        rt_task_fault(f, f->ptr_result);
    } else {
        rt_task_complete(f);
    }
}

/* RFC 008：leader 完成时级联扇出——遍历 follower 链，逐个传播结果并唤醒。
 * 由 rt_task_complete / rt_task_fault 末尾调用（cancel 后补 complete 的路径
 * 同样覆盖）。遍历中摘链清空，防二次扇出。
 *
 * Follower 链锁（case8 bounded_mpmc_stress 丢失唤醒根因）：add_follower 的
 * check-then-insert 与 complete 的 store READY → fire 跨线程无同步——add 读到
 * 过期 PENDING 后插链，leader 的 fire 已跑空 → follower 永挂（写者 await 永不
 * resume）。链上全部操作经本锁串行化：add 锁内 acquire 读 status（complete 的
 * release store 与锁序构成 happens-before，必见终态 → propagate）；complete/fault
 * 先 release store 终态再锁内摘链（add 插链晚于摘链时，其锁内读必见终态走
 * propagate；早于摘链时 f 在链上被 fire 覆盖）——两交错皆不丢。 */
static atomic_flag g_follower_lock = ATOMIC_FLAG_INIT;

static void rt_follower_lock(void) {
    while (atomic_flag_test_and_set_explicit(&g_follower_lock,
                                             memory_order_acquire)) {
        /* 自旋：follower 链操作极短（O(1) 摘/插），无长临界区 */
    }
}

static void rt_follower_unlock(void) {
    atomic_flag_clear_explicit(&g_follower_lock, memory_order_release);
}

static void rt_task_fire_followers(RtTask* t) {
    rt_follower_lock();
    RtTask* f = t->follower_head;
    t->follower_head = NULL;
    rt_follower_unlock();
    while (f) {
        RtTask* next = f->follower_next;
        f->follower_next = NULL;
        rt_task_propagate(t, f);
        f = next;
    }
}

/* RFC 008：get_Task 扇出注册。leader PENDING → follower 头插链表，leader
 * 完成时级联；leader 已完成（含缓存单例）→ 立即同步传播。follower 是
 * 独立 RtTask（await 单消费语义不动——消费 follower 不影响 leader 与
 * 其他 follower）。 */
void rt_task_add_follower(void* leader, void* follower) {
    RtTask* l = (RtTask*)leader;
    RtTask* f = (RtTask*)follower;
    if (!l || !f || l == f) return;
    rt_follower_lock();
    /* 锁内 acquire 读：与 complete/fault 的 release store（锁序同步）构成
     * happens-before——此处的 PENDING 判定不再受跨核可见性延迟污染。 */
    int32_t st = atomic_load_explicit((_Atomic(int32_t)*)&l->status,
                                      memory_order_acquire);
    if (st != RT_TASK_PENDING) {
        rt_follower_unlock();
        atomic_fetch_add_explicit(&g_diag_add_prop, 1, memory_order_relaxed);
        rt_task_propagate(l, f);
        return;
    }
    f->follower_next = l->follower_head;
    l->follower_head = f;
    rt_follower_unlock();
}

/* rt_task_adopt(dst, src)：把「已完成源 Task」的结果整体转移到「待完成目标
 * Task」并按源状态收口（READY → complete；FAULTED → fault），唤醒 dst 的
 * 等待者。结果字段所有权语义与源一致（int/ptr 借引用、value malloc 块、
 * FAULTED 异常的在途 +1）；先摘除源字段再 release，避免二次释放。
 * src 约定为 Task.FromResult / Task.FromException 产物（单发完成句柄）。 */
void rt_task_adopt(void* dst, void* src) {
    RtTask* d = (RtTask*)dst;
    RtTask* s = (RtTask*)src;
    if (!d) {
        if (s) rt_task_release(s);
        return;
    }
    if (!s || d == s) return;
    int32_t src_status = s->status;
    /* FromResult 缓存单例（from_slab == RT_TASK_FROM_CACHE）进程内共享：
     * 仅拷贝字段，绝不摘除、不 release——摘空会把共享单例的值永久污染为 0
     * （后续 FromResult 复用返回错值）。非缓存源走所有权整体转移。 */
    if (s->from_slab == RT_TASK_FROM_CACHE) {
        d->int_result = s->int_result;
        d->ptr_result = s->ptr_result;
        d->value_result = s->value_result;
        d->value_size = s->value_size;
        /* 缓存单例为值类型（int/bool），ptr_result 恒 NULL，标记位不适用。 */
        d->ptr_is_class = 0;
    } else {
        d->int_result = s->int_result;
        d->ptr_result = s->ptr_result;
        d->value_result = s->value_result;
        d->value_size = s->value_size;
        /* 所有权整体转移：标记位随结果一并转移（源摘除后 release 不再 dec）。 */
        d->ptr_is_class = s->ptr_is_class;
        s->int_result = 0;
        s->ptr_result = NULL;
        s->ptr_is_class = 0;
        s->value_result = NULL;
        s->value_size = 0;
        rt_task_release(s);
    }
    if (src_status == RT_TASK_FAULTED) {
        rt_task_fault(d, d->ptr_result);
    } else {
        rt_task_complete(d);
    }
}

/* RFC 009 M3：设置 env 析构函数指针。由 codegen 构造函数在 rt_task_from_state_machine 之后调用。 */
void rt_task_set_dtor_fn(void* state, void (*dtor_fn)(void* env)) {
    RtTask* t = (RtTask*)state;
    if (t) t->dtor_fn = dtor_fn;
}

/* RFC 009 M2: 状态机 resume 完成时，将结果写入 Task 句柄的标准 result 槽。
 * 由 codegen 生成的 resume 函数在 return Ready 前调用。
 * env 内部保存 task 反向指针，resume 通过它写入 result。 */
void rt_task_set_result_int(void* state, int32_t value) {
    RtTask* t = (RtTask*)state;
    if (t) t->int_result = value;
}

void rt_task_set_result_ptr(void* state, void* value) {
    RtTask* t = (RtTask*)state;
    if (t) t->ptr_result = value;
}

/* RFC 009 §结果所有权（强持有，2026-08-22 收敛）：class 结果专用写入。
 * codegen 在返回 ArcHeader class 结果时调用（区别于 string/array 走
 * rt_task_set_result_ptr 的借引用路径）。置 ptr_is_class=1，使 rt_task_release
 * 对结果统一 dec；调用方传 fresh/owned（+1）引用，所有权随之转给 task。 */
void rt_task_set_result_class(void* state, void* value) {
    RtTask* t = (RtTask*)state;
    if (t) {
        t->ptr_result = value;
        t->ptr_is_class = 1;
    }
}

void rt_task_set_result_value(void* state, void* data, int32_t size) {
    RtTask* t = (RtTask*)state;
    if (!t || !data || size <= 0) return;
    // 先分配新缓冲区，成功后再释放旧值——避免 OOM 时丢失已有数据
    void* new_buf = malloc((size_t)size);
    if (!new_buf) return;
    memcpy(new_buf, data, (size_t)size);
    if (t->value_result) free(t->value_result);
    t->value_result = new_buf;
    t->value_size = size;
}

void rt_task_set_waker(void* state, rt_waker* waker) {
    RtTask* t = (RtTask*)state;
    if (!t) return;
    rt_task_wk_lock(t);
    t->waker = waker;
    rt_task_wk_unlock(t);
}

void rt_waker_wake(rt_waker* waker) {
    if (waker && waker->wake) {
        waker->wake(waker->data);
    }
}

/* ---- waker 交接自旋锁（case2/case8 第三处丢失唤醒根治，2026-09-03）----
 *
 * 守卫对象：{t->waker, t->_waker_slot} 与 status 终态迁移的临界区。原实现中
 * register_waker 的 waker 安装（普通写）与 complete/fault 的 waker snapshot
 * （普通读）无互斥，check-after-register 只闭合「complete 整体先行」分支；
 * 并发交错（snapshot 漏读 waker → outer 的 status 复检读到 PENDING 挂起 →
 * READY release store 后至）即产生零唤醒源永挂——channels backpressure /
 * mpmc_stress 无探针时序下高频复现。锁内 {snapshot + READY store} 与
 * {install + status 复检} 互斥后，两交错（register 先 / complete 先）皆必
 * 观察到对方。 */
static void rt_cpu_pause(void); /* 定义于 rt_wait 区（919 行附近）；此处仅自旋退避 */

/* 唤醒链事件级 trace（取证）：四点统一序号，时间线可完整重构。
 * R=register(install) r=register(recheck 补发) C=complete(snapshot)
 * P=poll_inner 完成路径 W=coro_wake。限 30000 行。
 * 默认关闭（残余三态仍在观察面，复现时 ARC_DIAG=1 一键重开）——
 * 关闭态热路径开销为一次 relaxed load+分支。 */
static _Atomic(long long) g_diag_wk_seq;
static _Atomic(int32_t) g_diag_wk_trace_n;
static _Atomic(int) g_diag_enabled;

static int rt_diag_enabled(void) {
    int v = atomic_load_explicit(&g_diag_enabled, memory_order_relaxed);
    if (v < 0) {
        v = getenv("ARC_DIAG") != NULL;
        atomic_store_explicit(&g_diag_enabled, v, memory_order_relaxed);
    }
    return v;
}

static void rt_wk_trace(char kind, void* inner, void* outer, int32_t st) {
    if (!rt_diag_enabled()) {
        return;
    }
    if (atomic_fetch_add_explicit(&g_diag_wk_trace_n, 1,
                                  memory_order_relaxed) >= 30000) {
        return;
    }
    long long seq = atomic_fetch_add_explicit(&g_diag_wk_seq, 1,
                                              memory_order_relaxed);
    fprintf(stderr, "[WS] %c seq=%lld inner=%p outer=%p st=%d tid=%lu\n",
            kind, seq, inner, outer, st, (unsigned long)GetCurrentThreadId());
}

/* waker 交接自旋锁（rt_combinator.c 等跨 TU 共用；声明见 rt_abi.h） */
void rt_task_wk_lock(RtTask* t) {
    uint32_t expected = 0u;
    long spins = 0;
    while (!atomic_compare_exchange_strong_explicit(
            (_Atomic(uint32_t)*)&t->wk_lock, &expected, 1u,
            memory_order_acquire, memory_order_relaxed)) {
        expected = 0u;
        if (++spins == 200000000L) {
            rt_diag_btrace("wk_lock");
        }
        rt_cpu_pause();
    }
}

void rt_task_wk_unlock(RtTask* t) {
    atomic_store_explicit((_Atomic(uint32_t)*)&t->wk_lock, 0u,
                          memory_order_release);
}

/* ---- RFC 009 M3 新增 ---- */

/* 标记 Task 为 READY 并触发其 waker（将外层 Task 移入就绪队列）。
 * 由定时器到期、IO 完成、CTS 取消回调、worker trampoline 等外部事件调用。
 * 可从任意线程调用（waker 回调线程安全）。
 *
 * P0 teardown race 修复（2026-08-03）：
 * 原实现「先置 status=READY，后读 t->waker」存在 UAF 窗口——worker 线程置 READY
 * 后、读取 t->waker 前，EventLoop 线程（外层 await 的 resume）可能已观察到 READY
 * 并立即 rt_task_release → rt_task_slab_free → free(task)（e2e 未初始化 slab 时
 * 直接 free；slab 路径则把 t->waker 覆写为 free_list next）。此时 worker 再读
 * t->waker / t->_waker_slot（已释放/已复用内存）→ 以垃圾指针调用 wake →
 * 0xC0000374 堆损坏（async Main + ThreadPoolScheduler 拆卸期 flaky）。
 *
 * P0 二次修复（2026-08-03）：即使「先快照 waker，后置 READY」，若 status 写
 * 是普通 store，编译器/CPU 仍可能把 status=READY 重排到 t->waker=NULL 之前；
 * reader 观察到 READY 即 free(task)，worker 再写 t->waker → 写入已释放块 →
 * 堆损坏。故 status 必须是 **release store**：保证所有先前 store（含 waker 清空）
 * 在 READY 可见前已提交；reader 侧 rt_task_poll 用 acquire 读与之配对。 */
void rt_task_complete(void* state) {
    RtTask* t = (RtTask*)state;
    if (!t) return;
    void (*wake_fn)(void*) = NULL;
    void* wake_data = NULL;
    /* wk_lock 临界区：waker snapshot 与 READY store 对 register_waker 的
     * install + status 复检互斥（交接协议见 rt_task_wk_lock 注释） */
    rt_task_wk_lock(t);
    if (t->waker) {
        wake_fn = t->waker->wake;
        wake_data = t->waker->data;
        t->waker = NULL;
    } else {
        atomic_fetch_add_explicit(&g_diag_complete_no_waker, 1,
                                  memory_order_relaxed);
    }
    if (!t->canceled) {
        /* M4：已取消的 Task 保持 CANCELED 不被 READY 覆盖（但仍触发 waker
         * 唤醒 outer，由 rt_task_is_canceled 查询取消）。
         * release store：保证所有先前 store（含 waker 清空）在 READY 可见前
         * 已提交；reader 侧 rt_task_poll 用 acquire 读与之配对。 */
        atomic_store_explicit((_Atomic(int32_t)*)&t->status, RT_TASK_READY,
                              memory_order_release);
    }
    rt_task_wk_unlock(t);
    rt_wk_trace('C', t, wake_data,
                atomic_load_explicit((_Atomic(int32_t)*)&t->status,
                                     memory_order_relaxed));
    /* RFC 008：级联扇出（TCS get_Task 副本）。此时 leader 结果字段已就绪
    *（set_result_* 先于 complete 调用），propagate 读取一致快照。 */
    if (t->follower_head) {
        rt_task_fire_followers(t);
    }
    /* 触发 waker（唤醒外层 await 此 Task 的 Task）；仅用已快照的局部值。 */
    if (wake_fn) {
        wake_fn(wake_data);
    }
}

/* rt_task_fault: 将 Task 标记为 FAULTED 并存入异常对象，然后触发 waker。
 * 由异步边界（C trampoline 捕获 Arc 异常后）调用——异常写入 ptr_result，
 * 状态置 FAULTED（release store，与 rt_task_complete 同策防 teardown race），
 * await/Wait 侧经 rt_task_is_faulted + rt_task_get_exception 读取并 rethrow。
 * 异常对象借用「throw 在途 +1」引用，不单独 inc（与 rt_task_from_exception
 * 契约一致；await 提取时由 codegen rt_arc_inc 授予 catch 绑定独立引用）。 */
void rt_task_fault(void* state, void* exception) {
    RtTask* t = (RtTask*)state;
    if (!t) return;
    void (*wake_fn)(void*) = NULL;
    void* wake_data = NULL;
    /* wk_lock 临界区：与 complete 同策（waker snapshot / FAULTED store 对
     * register_waker 互斥） */
    rt_task_wk_lock(t);
    if (t->waker) {
        wake_fn = t->waker->wake;
        wake_data = t->waker->data;
        t->waker = NULL;
    }
    if (!t->canceled) {
        t->ptr_result = exception;
        /* 异常恒为 Arc class：置 ptr_is_class=1 使 release 统一 dec
         * （RFC 009 §结果所有权 强持有）。 */
        t->ptr_is_class = 1;
        /* release store：保证 ptr_result 可见前，所有先前 store（t->waker=NULL
         * 等）已提交，避免 reader free 后 worker 残余写。 */
        atomic_store_explicit((_Atomic(int32_t)*)&t->status, RT_TASK_FAULTED,
                              memory_order_release);
    }
    rt_task_wk_unlock(t);
    /* RFC 008：级联扇出——FAULTED 传播（每 follower 独立 inc 在途引用）。 */
    if (t->follower_head) {
        rt_task_fire_followers(t);
    }
    if (wake_fn) {
        wake_fn(wake_data);
    }
}

/* M6.2 异步 waker：先清「暖启动守卫位」再投递 outer 到调度器。
 *
 * 守卫协议（await_waiting ∈ {0,1}，原子访问）：
 *   - await 挂起：rt_task_register_waker 置 outer->await_waiting=1 ——
 *     「outer 正依赖 inner 完成唤醒才续行」。
 *   - 父 await poll：见位（非 0）即返 RT_TASK_PENDING（绝不异地 resume 未完成
 *     body——暖启动路径下父 poll 若越过守卫越界 resume，二次 resume 误判 final
 *     → 子成孤儿 → 0xC0000005）。
 *   - waker 触发：本回调（data=outer，恒与「inner 已完成」配对）先清 outer
 *     守卫位再投递——此后外层 poll 才越过守卫（恒 waker 驱动，闭合「wake 先于
 *     置位」竞态）。
 *   - 非 await 场景（autostart 首 poll 直接越守），autostart 内部见位先清 0
 *     再推进（一次性守卫）。
 *
 * 非协程路径（状态机/默认 dispatch）也复用本回调：清位 + 投递是 g_rt_wake_fn
 * 的严格超集（位恒 0 时零差异），故 rt_task_register_waker 统一挂本回调。 */
static void rt_task_coro_wake(void* data) {
    atomic_fetch_add_explicit(&g_diag_coro_wake, 1, memory_order_relaxed);
    rt_wk_trace('W', data, NULL, -1);
    if (data) {
        atomic_store_explicit((_Atomic(uint32_t)*)&((RtTask*)data)->await_waiting,
                              0u, memory_order_release);
    }
    if (g_rt_wake_fn) {
        g_rt_wake_fn(data);
    }
}

/* 为 inner Task 注册默认 waker：wake 时将 outer Task 移入 EventLoop/线程池就绪队列。
 * 在状态机/协程 await 时调用：poll inner → PENDING → register_waker(inner, outer) → ret PENDING。
 * 使用 Task 内嵌的 _waker_slot，避免堆分配 waker binding。
 *
 * M6.2 暖启动守卫：本调用同时置 outer->await_waiting=1——outer 挂起等待 inner
 * 完成，此后任何对 outer 的异地 poll（其父 await）见位即返 PENDING，绝不越界
 * resume；inner 完成时经 rt_task_coro_wake（内嵌槽 wake 回调）清位后再投递 outer。 */
void rt_task_register_waker(void* inner_task, void* outer_task) {
    RtTask* inner = (RtTask*)inner_task;
    if (!inner || !outer_task) return;
    /* RFC 009 下钻：await 路径在此注册 waker 后挂起，依赖 Task 被调度才能完成。
     * 若 Task 仍滞留于当前线程生产者批（< RT_TP_BATCH 未冲刷），将永不发布 → await
     * 永久挂起。故注册 waker 前必须先冲刷批，保证「已 spawn 必已发布」。 */
    rt_threadpool_flush_local();
    /* 设置 waker 槽：wake 回调调用 rt_event_loop_spawn。
     * 但该函数在 rt_event_loop.c 中定义。为避免循环依赖，
     * 使用全局函数指针，由 rt_event_loop.c 在 create 时设置。
     * M6.2：wake 回调改为 rt_task_coro_wake——先清 outer 守卫位再投递。 */
    atomic_store_explicit((_Atomic(uint32_t)*)&((RtTask*)outer_task)->await_waiting,
                          1u, memory_order_release);
    /* wk_lock 临界区：install 与终态复检对 complete/fault 的 snapshot + 终态
     * store 互斥——两交错皆不丢唤醒（交接协议见 rt_task_wk_lock 注释）：
     *   - register 先：complete 锁内 snapshot 必见 waker → 正常唤醒；
     *   - complete 先：register 锁内复检必见终态 → waker 回收 + 补发。
     * 旧实现两侧均普通读写，仅靠锁外 acquire 复检闭合「complete 整体先行」，
     * 「snapshot 与 install 并发」交错即丢失（snapshot 漏读 → outer 挂起 →
     * READY 后至 → 零唤醒源永挂）。 */
    atomic_store_explicit(&g_diag_reg_in_flight, 1, memory_order_relaxed);
    g_diag_reg_inner = inner;
    rt_task_wk_lock(inner);
    atomic_store_explicit(&g_diag_reg_in_flight, 2, memory_order_relaxed);
    inner->_waker_slot.data = outer_task;
    if (g_rt_wake_fn) {
        inner->_waker_slot.wake = rt_task_coro_wake;
    } else {
        /* fallback：无 EventLoop 时 waker 无效 */
        inner->_waker_slot.wake = NULL;
    }
    inner->waker = &inner->_waker_slot;
    int32_t st = atomic_load_explicit((_Atomic(int32_t)*)&inner->status,
                                      memory_order_acquire);
    rt_wk_trace('R', inner, outer_task, st);
    /* check-after-register（原语义保留，现于锁内）：闭合「注册前完成」唤醒
     * 丢失窗口。await 挂起序列为 poll(PENDING) → register_waker，两步之间夹
     * 两次 flush_local（poll 入口与本函数入口），窗口可达微秒级且在高负载下
     * 被调度抢占进一步拉宽；若 inner（IO/worker 完成侧）恰在此窗口置终态，
     * complete 读 waker 尚为 NULL、不触发唤醒，outer 永挂（net_tcp_echo_async
     * 高负载 flaky 复现，§7.5 竞态谱系）。已终态则回收 waker 并于锁外经
     * coro_wake 补发唤醒——清 outer 守卫位 + 投递就绪队列，与「complete 在
     * 注册后」场景共用同一 waker 通道；投递消费必在状态机 save_locals/state
     * 落盘之后（register_waker 返回前状态机尚未 ret PENDING，重 poll 由
     * POLLING CAS + NOTIFIED 协议去重）。未终态则按原路径挂起，位恒幂等零开销。 */
    if (st != RT_TASK_PENDING) {
        inner->waker = NULL;
    }
    rt_task_wk_unlock(inner);
    atomic_store_explicit(&g_diag_reg_in_flight, 0, memory_order_relaxed);
    if (st != RT_TASK_PENDING) {
        atomic_fetch_add_explicit(&g_diag_reg_late, 1, memory_order_relaxed);
        rt_wk_trace('r', inner, outer_task, st);
        rt_task_coro_wake(outer_task);
    }
}

/* 取证读取：poll 相位 + register 在途状态（rt_threadpool.c 转储调用，临时） */
void rt_diag_poll_state(unsigned long long* out) {
    out[0] = atomic_load_explicit(&g_diag_poll_phase, memory_order_relaxed);
    out[1] = (unsigned long long)(uintptr_t)g_diag_poll_task;
    out[2] = atomic_load_explicit(&g_diag_reg_in_flight, memory_order_relaxed);
    out[3] = (unsigned long long)(uintptr_t)g_diag_reg_inner;
}

/* M6.2 协程暖启动：async 函数调用点发射（对标 .NET async 同步前缀）。
 *
 * 冷启动缺陷：协程/SM async 任务创建后需等待首次 poll 才驱动 body，导致
 * 「create N → await each」模式下任务串行执行（ExecutorStressTests 观测
 * maxSeen=1、耗时逼近串行下界）。本函数在任务创建后立即首 poll，驱动 body
 * 同步前缀至首个未完成 await——任务创建即开始执行，N 个任务可并行推进。
 * 首 poll 前先清位（非 await 场景见位先清 0 再推进，一次性守卫），幂等零开销。
 *
 * 不做 post-poll 重置位（6b 唤醒丢失修复）：poll 返 PENDING 时守卫位必为
 * 二者之一，重置位要么冗余要么有害——
 *   - 位仍为 1：body 挂起于未完成 inner await（register_waker 置位后无
 *     coro_wake 清除），守卫已在位，重置位幂等冗余；
 *   - 位为 0：coro_wake 已清位（register_waker check-after-register 补发，
 *     或 inner 完成侧 IO/worker 线程触发）——coro_wake 清位后必投递本任务，
 *     投递在途驱动下一次 poll；重置位将令该投递被 poll 顶部门控丢弃，而
 *     inner 已终态再无唤醒源 → 本任务与父任务永挂（net_tcp_echo_async
 *     高负载概率复现，同二进制 FAIL→PASS→PASS 采样实证）。
 * 不变量「PENDING ⇒ 位 1 ∨ 投递在途」由 register_waker/coro_wake 协议自身
 * 维持（poll_inner 仅在 st != PENDING 完成路径清位，PENDING 返回路径不触碰
 * 守卫位），故 post-poll 置位删除。 */
void rt_task_autostart(void* state) {
    RtTask* t = (RtTask*)state;
    if (!t) return;
    atomic_store_explicit((_Atomic(uint32_t)*)&t->await_waiting, 0u,
                          memory_order_release);
    rt_task_poll(state);
}

/* ---- RFC 009 M5.7: Wait / WaitAll / WaitAny / FromCanceled 新增 ---- */

#include <time.h>
#ifdef _WIN32
  #ifndef WIN32_LEAN_AND_MEAN
    #define WIN32_LEAN_AND_MEAN
  #endif
  #include <windows.h>
#else
  #include <unistd.h>
  #include <pthread.h>
  #include <sched.h>
#endif

/* 平台无关 sleep（毫秒级） */
static void rt_sleep_ms(int32_t ms) {
    if (ms <= 0) return;
#ifdef _WIN32
    Sleep((DWORD)ms);
#else
    usleep((useconds_t)ms * 1000);
#endif
}

/* ---------- 单 Task Wait 自适应等待原语（RFC 038 对齐） ----------
 *
 * 业界对标：.NET Task.Wait 内部用 SpinWait（先短暂自旋，成功即返回，
 * 失败才递减到 yield/阻塞）；Go runtime 对 channel 等待亦先自旋再 park。
 * Arc 单 Task Wait（wait_timeout/wait_ct/wait_any）原实现为固定
 * rt_sleep_ms(1)——worker 在 µs 级完成，等待者却付 ms 级 sleep，属结构
 * 性缺陷（wait_all 已 condvar 化消除，单 Task 未对齐）。
 *
 * 本方案：自旋 → yield → 渐进退避 三阶段。worker 多 µs 级完成，自旋
 * （PAUSE 级，~几十 μs）即可零延迟覆盖绝大多数场景；spin 耗尽后 yield
 * 让出 CPU 给 worker；再退避 sleep 防饿死。不注册栈上 waker/condvar ctx
 * ——规避单 Task timeout 晚完成写回已释放栈的 UAF（wait_all 靠 count 归
 * 零作线性化点保障安全，单 Task timeout 无法同样保证）。
 */
#define RT_WAIT_SPIN_MAX  10000   /* PAUSE 自旋次数（~几十 μs） */
#define RT_WAIT_YIELD_MAX 100     /* yield 让出次数 */

typedef struct rt_wait_relax {
    int32_t spin_left;
    int32_t yield_left;
} rt_wait_relax;

static void rt_wait_relax_init(rt_wait_relax* r) {
    r->spin_left = RT_WAIT_SPIN_MAX;
    r->yield_left = RT_WAIT_YIELD_MAX;
}

/* PAUSE：x86 自旋时降低功耗/总线竞争；非 x86 退化为空转（仍达自旋目的） */
static void rt_cpu_pause(void) {
#if defined(_MSC_VER) && defined(_M_X64)
    _mm_pause();
#elif defined(__x86_64__) || defined(__i386__)
    __builtin_ia32_pause();
#else
    /* 非 x86：空转即自旋 */
#endif
}

/* yield：让出 CPU 给其他可运行线程（worker） */
static void rt_thread_yield(void) {
#ifdef _WIN32
    SwitchToThread();
#else
    sched_yield();
#endif
}

/* 自适应步进：自旋 → yield → sleep(1) 退避 */
static void rt_wait_relax_step(rt_wait_relax* r) {
    if (r->spin_left > 0) {
        r->spin_left--;
        rt_cpu_pause();
    } else if (r->yield_left > 0) {
        r->yield_left--;
        rt_thread_yield();
    } else {
        rt_sleep_ms(1);
    }
}

/* ---- RFC 038（2026-08-07）：rt_task_wait_all condvar+计数器化 ----
 *
 * 业界顶级方案参照：.NET Task.WaitAll（condvar + 计数器，无轮询）、
 * Go sync.WaitGroup（sema 计数信号量）。
 *
 * 原实现缺陷：rt_sleep_ms(1) 轮询——burst spawn 场景（N=50000）每轮 poll 后
 * sleep 1ms，worker 在 µs 级完成但等待者付 ms 级 sleep → ~30ms 纯 sleep 开销
 * （占 task_spawn_wait ~730ns/op 的大头）。根因非"共享机噪声"，是轮询+sleep
 * 的结构性缺陷。
 *
 * 新设计：每个 pending Task 注册 waker → complete 时 fetch_sub count；
 * count 归零时 condvar signal，wait_all 立即返回。无 sleep 轮询。
 *
 * 正确性保证：
 *   - wait_all 持锁检查 count>0 才 wait（避免 lost wakeup：worker signal 前持锁）
 *   - count==0 时所有 wake_fn 已返回（fetch_sub 是线性化点），wakers/ctx 可安全释放
 *   - OOM 回退：自旋 + yield（rt_sleep_ms(0)），不 sleep(1) */

/* 平台无关当前时间（毫秒） */
static int64_t rt_now_ms(void) {
#ifdef _WIN32
    return (int64_t)GetTickCount64();
#else
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (int64_t)ts.tv_sec * 1000 + ts.tv_nsec / 1000000;
#endif
}

/* rt_task_wait_timeout: 同步轮询等待 Task 完成，带超时。
 * timeout_ms == 0 → 无限等待。
 * 返回 1=完成，0=超时，-1=已取消。 */
int32_t rt_task_wait_timeout(void* state, int32_t timeout_ms) {
    RtTask* t = (RtTask*)state;
    if (!t) return 1;
    /* RFC 009 下钻：冲刷当前线程生产者批，闭合「同线程 spawn 后 wait」尾批未发布。 */
    rt_threadpool_flush_local();
    int64_t deadline = (timeout_ms > 0) ? rt_now_ms() + timeout_ms : 0;
    rt_wait_relax relax;
    rt_wait_relax_init(&relax);

    while (1) {
        int32_t st = rt_task_poll(state);
        if (t->canceled) return -1;
        if (st != RT_TASK_PENDING) return 1;
        if (timeout_ms > 0 && rt_now_ms() >= deadline) return 0;
        rt_wait_relax_step(&relax); /* 自旋→yield→退避，替代固定 sleep(1) */
    }
}

/* rt_task_wait_ct: 同步轮询等待 Task 完成，可被 CancellationToken 中断。
 * 返回 1=完成，0=被取消中断。
 * C# TPL 语义：已完成 Task 无论 CT 状态都立即返回 true；
 * PENDING Task 每轮 poll 后再检查 CT，CT 已取消则返回 0。 */
int32_t rt_task_wait_ct(void* state, void* ct) {
    RtTask* t = (RtTask*)state;
    if (!t) return 1;
    /* RFC 009 下钻：冲刷当前线程生产者批，闭合「同线程 spawn 后 wait」尾批未发布。 */
    rt_threadpool_flush_local();
    rt_wait_relax relax;
    rt_wait_relax_init(&relax);

    while (1) {
        int32_t st = rt_task_poll(state);
        if (t->canceled) return 0;
        if (st != RT_TASK_PENDING) return 1;
        if (ct && rt_cts_is_canceled(ct)) return 0;
        rt_wait_relax_step(&relax); /* 自旋→yield→退避，替代固定 sleep(1) */
    }
}

/* rt_task_wait_all: 同步阻塞等待全部 Task 完成。
 * 轮询所有 tasks，全部非 PENDING 时返回。 */
/* wait_all 共享上下文：计数器 + condvar（参照 .NET Task.WaitAll 内部结构）。
 * 栈上分配（wait_all 调用方持有），生命周期覆盖所有 wake_fn 调用。 */
typedef struct rt_wait_ctx {
    _Atomic(int32_t) count;  /* 剩余未完成 Task 数 */
#ifdef _WIN32
    CRITICAL_SECTION mutex;
    CONDITION_VARIABLE cond;
#else
    pthread_mutex_t mutex;
    pthread_cond_t  cond;
#endif
} rt_wait_ctx;

static void rt_wait_ctx_init(rt_wait_ctx* c) {
    atomic_init(&c->count, 0);
#ifdef _WIN32
    InitializeCriticalSection(&c->mutex);
    InitializeConditionVariable(&c->cond);
#else
    pthread_mutex_init(&c->mutex, NULL);
    pthread_cond_init(&c->cond, NULL);
#endif
}

static void rt_wait_ctx_destroy(rt_wait_ctx* c) {
#ifdef _WIN32
    DeleteCriticalSection(&c->mutex);
#else
    pthread_mutex_destroy(&c->mutex);
    pthread_cond_destroy(&c->cond);
#endif
}

/* 平台锁/唤醒封装：wait_all 的「持锁注册 + 持锁检查 count + wait」与 wake_fn 的
 * 「持锁递减 + signal」共用，闭合 lost wakeup（见 rt_task_wait_all 说明）。 */
static void rt_wait_ctx_lock(rt_wait_ctx* c) {
#ifdef _WIN32
    EnterCriticalSection(&c->mutex);
#else
    pthread_mutex_lock(&c->mutex);
#endif
}
static void rt_wait_ctx_unlock(rt_wait_ctx* c) {
#ifdef _WIN32
    LeaveCriticalSection(&c->mutex);
#else
    pthread_mutex_unlock(&c->mutex);
#endif
}
static void rt_wait_ctx_signal(rt_wait_ctx* c) {
#ifdef _WIN32
    WakeConditionVariable(&c->cond);
#else
    pthread_cond_broadcast(&c->cond);
#endif
}

/* wait_all 的 per-task 完成计数项。
 * waker 指向本项（data = &entry），完成回调经它访问 consumed 标志 + 共享 ctx。
 * consumed 保证「每个 Task 恰好递减一次 count」——闭合 lost wakeup（见 rt_task_wait_all）。 */
typedef struct rt_wait_entry {
    rt_waker        waker;      /* 注册到 Task 的 waker（须含 wake+data 两个字段） */
    rt_wait_ctx*    ctx;        /* 共享计数器 + condvar */
    _Atomic(int32_t) consumed;  /* 0=未递减；1=已递减（worker 或 wait_all 之一执行一次） */
} rt_wait_entry;

/* wait_all 的 waker 回调：Task complete 时调用，fetch_sub count。
 * 最后一个 Task（prev==1）时 condvar signal 唤醒等待者。
 *
 * 递减本身无锁（consumed 0→1 CAS 幂等，与 wait_all 的 re-poll 手动递减互斥）；
 * 仅最后一个递减（prev==1）取锁 signal——与 wait_all 的「持锁检查 count + wait」
 * 配对闭合 final lost wakeup。注册期 count 用 fetch_add 实时递增（非 store 覆盖），
 * 故此处递减不会与 count 设终值竞争而丢失。
 *
 * 幂等性：consumed 0→1 CAS 保证每个 Task 恰好递减一次。注册期已完成的 Task 由
 * wait_all 在 re-poll 手动递减（置 consumed=1），此处 CAS 失败即跳过。 */
static void rt_wait_all_wake_fn(void* data) {
    rt_wait_entry* e = (rt_wait_entry*)data;
    rt_wait_ctx* ctx = e->ctx;
    int32_t expected = 0;
    if (!atomic_compare_exchange_strong_explicit(&e->consumed, &expected, 1,
            memory_order_acq_rel, memory_order_relaxed)) {
        return;  /* 已被 wait_all 手动递减（注册期完成的 Task），跳过 */
    }
    int32_t prev = atomic_fetch_sub_explicit(&ctx->count, 1, memory_order_acq_rel);
    if (prev == 1) {
        rt_wait_ctx_lock(ctx);
        rt_wait_ctx_signal(ctx);  /* 最后一个 Task 完成 → 唤醒等待者（持锁 signal） */
        rt_wait_ctx_unlock(ctx);
    }
}

/* rt_task_wait_all: 同步阻塞等待全部 Task 完成。
 * RFC 038（2026-08-07）：condvar + 计数器化，消除原 rt_sleep_ms(1) 轮询。
 * 参照 .NET Task.WaitAll / Go WaitGroup。 */
void rt_task_wait_all(void** tasks, int32_t count) {
    if (!tasks || count <= 0) return;

    /* RFC 009 下钻：冲刷当前线程生产者批——spawn 循环的尾批（< RT_TP_BATCH）
     * 滞留于 TLS 批未发布，不冲刷则 poll 恒 PENDING、wait 空转。 */
    rt_threadpool_flush_local();

    /* 快速路径：先 poll 一轮，若全部完成则直接返回（避免 condvar 初始化开销）。
     * burst 场景第一批可能已部分完成。同时统计非空任务数 nvalid（慢路径用）。
     * **下钻优化（2026-08-07）**：count 单次 store = nvalid，消除注册期每任务
     * fetch_add（50000 次 RMW → 1 次 store）。 */
    int all_done = 1;
    int32_t nvalid = 0;
    for (int32_t i = 0; i < count; i++) {
        if (!tasks[i]) continue;
        nvalid++;
        if (rt_task_poll(tasks[i]) == RT_TASK_PENDING) all_done = 0;
    }
    if (all_done) return;

    /* 慢路径：condvar + 计数器 */
    rt_wait_ctx ctx;
    rt_wait_ctx_init(&ctx);

    /* **下钻优化（2026-08-07）**：count 先置 nvalid（release store，仅 1 次），
     * 后逐任务置 waker。据此每个 Task 恰好递减一次（consumed CAS 幂等）：
     *   - 已完成 Task（complete 早于 waker 置位）→ complete 读 waker==NULL 无
     *     wake_fn 递减，由下方 poll-pass 手动递减（CAS 0→1 成功）；
     *   - PENDING Task → 注册后 complete 触发 wake_fn 递减（CAS 0→1 成功）。
     * 因 waker 置位在 count store 之后，且 wake_fn 仅经 waker 可达，故 wake_fn
     * 必在 count=nvalid 可见后才递减——不会被后续 store 覆盖（原实现 fetch_add
     * 即防此覆盖；单次 store 前置亦闭合该竞态）。 */
    atomic_store_explicit(&ctx.count, nvalid, memory_order_release);

    /* 每个 pending Task 需一个 rt_wait_entry（内含 waker + consumed 标志）。
     * 数组按 count（入参）索引，故按其分配；OOM 回退自旋+yield。 */
    rt_wait_entry* entries = (rt_wait_entry*)calloc((size_t)count, sizeof(rt_wait_entry));
    if (!entries) {
        /* OOM 回退：自旋 + yield（不 sleep_ms(1)，用 sleep_ms(0)=yield） */
        while (1) {
            int done = 1;
            for (int32_t i = 0; i < count; i++) {
                if (!tasks[i]) continue;
                if (rt_task_poll(tasks[i]) == RT_TASK_PENDING) done = 0;
            }
            if (done) break;
            rt_sleep_ms(0);  /* yield，非 sleep(1) */
        }
        rt_wait_ctx_destroy(&ctx);
        return;
    }

    /* 为全部非空 Task 注册 waker（对已完成 Task 设置 waker 无害——complete 已返回，
     * 无人再读 waker）。count 已在置 waker 前 store=nvalid，故此循环不再递增 count。 */
    for (int32_t i = 0; i < count; i++) {
        if (!tasks[i]) continue;
        entries[i].waker.wake = rt_wait_all_wake_fn;
        entries[i].waker.data = &entries[i];
        entries[i].ctx = &ctx;
        atomic_init(&entries[i].consumed, 0);
        rt_task_set_waker(tasks[i], &entries[i].waker);
    }

    /* 闭合 lost wakeup：单次 poll-pass，凡已离开 PENDING 的 Task 手动递减一次
     * （consumed 0→1 CAS 幂等，避免与 wake_fn 双递减）。
     *    - 注册前已完成的 Task：complete 读到 waker==NULL 未递减 → 此处 CAS 成功，手动递减；
     *    - 注册后由 wake_fn 递减的 Task：consumed 已被置 1 → 此处 CAS 失败，跳过；
     *    - 仍 PENDING 的 Task：留待后续 complete 触发 wake_fn 递减。
     * 保证每个计入 count 的 Task 恰好递减一次，count 必达 0。 */
    for (int32_t i = 0; i < count; i++) {
        if (!tasks[i]) continue;
        if (rt_task_poll(tasks[i]) != RT_TASK_PENDING) {
            int32_t expected = 0;
            if (atomic_compare_exchange_strong_explicit(&entries[i].consumed, &expected, 1,
                    memory_order_acq_rel, memory_order_relaxed)) {
                atomic_fetch_sub_explicit(&ctx.count, 1, memory_order_acq_rel);
            }
        }
    }

    /* count>0 才 wait。持锁检查 + cond_wait 与 wake_fn 的「prev==1 持锁 signal」
     * 配对闭合 final lost wakeup。 */
    rt_wait_ctx_lock(&ctx);
    while (atomic_load_explicit(&ctx.count, memory_order_acquire) > 0) {
#ifdef _WIN32
        SleepConditionVariableCS(&ctx.cond, &ctx.mutex, INFINITE);
#else
        pthread_cond_wait(&ctx.cond, &ctx.mutex);
#endif
    }
    rt_wait_ctx_unlock(&ctx);

    /* count==0：所有 wake_fn 已返回（fetch_sub 线性化点），entries/ctx 可安全释放 */
    free(entries);
    rt_wait_ctx_destroy(&ctx);
}

/* rt_task_wait_any: 同步阻塞等待任一 Task 完成，返回该 Task 索引。
 * 所有 Task 已完成时返回第一个非 PENDING 的索引。 */
int32_t rt_task_wait_any(void** tasks, int32_t count) {
    if (!tasks || count <= 0) return -1;

    /* RFC 009 下钻：冲刷当前线程生产者批，闭合「同线程 spawn 后 wait」尾批未发布。 */
    rt_threadpool_flush_local();
    rt_wait_relax relax;
    rt_wait_relax_init(&relax);

    while (1) {
        for (int32_t i = 0; i < count; i++) {
            if (!tasks[i]) continue;
            int32_t st = rt_task_poll(tasks[i]);
            if (st != RT_TASK_PENDING) return i;
        }
        rt_wait_relax_step(&relax); /* 自旋→yield→退避，替代固定 sleep(1) */
    }
}

/* rt_task_from_canceled: 创建已取消的 Task（状态 CANCELED，无结果）。
 * 由 Task.FromCanceled / Task<T>.FromCanceled 工厂方法调用。 */
void* rt_task_from_canceled(void) {
    RtTask* t = rt_task_alloc();
    if (t) {
        t->canceled = 1;
        t->status = RT_TASK_CANCELED;
    }
    return t;
}

/* rt_task_from_exception: 创建已失败的 Task（状态 FAULTED；异常对象存 ptr_result）。
 * 由 Task.FromException / Task<T>.FromException 工厂方法调用。 */
void* rt_task_from_exception(void* exception) {
    RtTask* t = rt_task_alloc();
    if (t) {
        t->status = RT_TASK_FAULTED;
        t->ptr_result = exception;
        /* 异常恒为 Arc class：置位使 release 统一 dec（强持有）。 */
        t->ptr_is_class = 1;
    }
    return t;
}

int32_t rt_task_is_faulted(void* state) {
    RtTask* t = (RtTask*)state;
    if (!t) return 0;
    return (!t->canceled && t->status == RT_TASK_FAULTED) ? 1 : 0;
}

void* rt_task_get_exception(void* state) {
    RtTask* t = (RtTask*)state;
    if (!t || t->canceled || t->status != RT_TASK_FAULTED) return NULL;
    return t->ptr_result;
}
