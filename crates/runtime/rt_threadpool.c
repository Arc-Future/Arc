// ThreadPoolScheduler (RFC 009 M5.1).
//
// N worker 线程 + per-worker Chase-Lev work-stealing deque + 全局 injector queue。
//
// 调度策略（RFC 009 §16.3）：
//   1. Worker 醒来 → pop 本地 LIFO（cache 局部性最优）
//   2. 本地空 → steal 其他 worker 的 FIFO 端（窃取最旧任务，负载均衡）
//   3. 全部本地空 → 拉取 Global Injector
//   4. 全部空 → park（condvar），由 push 唤醒
//
// M5.1 MVP 决策：
//   - 全局队列用 mutex + 链表（M5.2 升级为 MPSC 无锁）
//   - park/wake 用 pool 级 mutex + condvar（M5.2 升级为 futex/WaitOnAddress）
//   - worker 数固定（不动态伸缩）
//   - 不含 NUMA 感知（M5.7 / RFC 009 实现）
//
// 性能目标（RFC 009 §16.4）：
//   - push/pop: O(1), 无 CAS, ~5ns（热路径无锁）
//   - steal: O(1), 1 次 CAS, ~30ns
//   - spawn 跨 worker: 随机选 victim → steal，<30ns 调度延迟

#include "rt_abi.h"
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifdef _WIN32
  #include <windows.h>
  #include <immintrin.h> /* _mm_pause（wait_idle 快排） */
  #include <process.h> /* _beginthreadex：CRT 安全，避免进程退出期 flaky AV */
  typedef HANDLE    rt_thread_t;
#else
  #include <pthread.h>
  #include <unistd.h> /* sysconf(_SC_NPROCESSORS_ONLN) */
  #include <time.h>
  typedef pthread_t rt_thread_t;
#endif

/* 有界自旋退避（Vyukov injector 的 push/pop 占位等待 + worker 自旋复用）。
 * x86 用 _mm_pause（省功耗 + 防流水线风暴）；非 x86 空操作（由原子 load 本身退避）。 */
#if defined(_M_X64) || defined(_M_IX86) || defined(__x86_64__) || defined(__i386__)
  #if !defined(_MSC_VER) && !defined(__AVX__)
    #include <immintrin.h>
  #endif
  #define RT_CPU_RELAX() _mm_pause()
#else
  #define RT_CPU_RELAX() ((void)0)
#endif

/* worker park 前有界自旋窗口（RFC 009 M5 ws 派发预算 · 2026-08-04）：
 * 轻量轮询 ~3ns/轮，2048 轮 ≈ 数 µs。burst 场景吸收后续 spawn（避免每任务
 * 一 park/wake 乒乓）；单任务后最多延迟 RT_TP_SPIN_LIMIT 轮才进 park。 */
#define RT_TP_SPIN_LIMIT 2048

/* 平台无关当前时间（毫秒）—— 与 rt_task.c 的 rt_now_ms 一致。
 * ThreadPool 不依赖 rt_task.c（避免循环依赖层），此处独立实现。 */
static int64_t rt_tp_now_ms(void) {
#ifdef _WIN32
    return (int64_t)GetTickCount64();
#else
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (int64_t)ts.tv_sec * 1000 + ts.tv_nsec / 1000000;
#endif
}

/* ---- 平台抽象（与 rt_event_loop.c 一致风格） ---- */

/* 丢失唤醒取证诊断计数器（临时：定义于 rt_event_loop.c，定位后整体回收） */
extern _Atomic(uint64_t) g_diag_wake_calls;
extern _Atomic(uint64_t) g_diag_wake_drop_el;
extern _Atomic(uint64_t) g_diag_wake_drop_data;
extern _Atomic(uint64_t) g_diag_coro_wake;
extern _Atomic(uint64_t) g_diag_poll_work;
extern _Atomic(uint64_t) g_diag_reg_late;
extern _Atomic(uint64_t) g_diag_add_prop;
extern _Atomic(uint64_t) g_diag_el_ticks;
extern _Atomic(uint32_t) g_diag_el_pending;
extern _Atomic(uint32_t) g_diag_el_ready;
extern _Atomic(long) g_diag_el_mutex_owner;
extern _Atomic(long) g_diag_el_mutex_waiters;
extern _Atomic(long) g_diag_el_tid;
extern _Atomic(uint32_t) g_diag_el_phase;
extern _Atomic(uint32_t) g_diag_el_has_reactor;
static _Atomic(uint64_t) g_diag_inject_push;
static _Atomic(uint64_t) g_diag_inject_pop;
static _Atomic(uint64_t) g_diag_park_waits;
static _Atomic(uint64_t) g_diag_hb_wakes;

/* 活任务普查（rt_task_slab.c 定义，临时取证） */
void rt_diag_task_census(unsigned long long* out);

/* Monitor 死锁图转储（rt_thread.c 定义，临时取证） */
void rt_mon_diag_dump(void);

/* poll 相位 / register 在途读取（rt_task.c 定义，临时取证） */
void rt_diag_poll_state(unsigned long long* out);

/* POLLING 持有者审计表转储（rt_task.c 定义，临时取证） */
void rt_diag_poll_own_dump(void);

/* acquire 环形栈缓冲转储（rt_task.c 定义，临时取证） */
void rt_diag_ring_dump(void);

/* 跨线程取栈（临时取证）：SuspendThread + StackWalk64 + PDB 符号化。
 * 用于解析 Monitor 持有者在临界区内的确切阻塞点。 */
void rt_diag_thread_stack(unsigned long tid, const char* tag);

/* poll 进入数 / re-poll 迭代数（rt_task.c 定义，活锁取证） */
extern _Atomic(uint64_t) g_diag_poll_entries;
extern _Atomic(uint64_t) g_diag_repoll_iters;
extern _Atomic(uint64_t) g_diag_complete_no_waker;

/* ---- 自捕获栈（临时取证）：自旋超限时打印调用栈（dbghelp 运行时加载，
 * 无链接依赖；PDB 与 DLL 同目录时符号化）。一次性触发防刷屏。 ---- */
void rt_diag_btrace(const char* tag) {
    static _Atomic(uint32_t) taken;
    uint32_t exp = 0;
    if (!atomic_compare_exchange_strong_explicit(&taken, &exp, 1u,
            memory_order_acq_rel, memory_order_relaxed)) {
        return;
    }
    HMODULE dbgh = LoadLibraryA("dbghelp.dll");
    if (!dbgh) {
        fprintf(stderr, "[btrace] %s: dbghelp unavailable\n", tag);
        return;
    }
    typedef BOOL(WINAPI* SymInitializeFn)(HANDLE, PCSTR, BOOL);
    typedef BOOL(WINAPI* SymFromAddrFn)(HANDLE, DWORD64, PDWORD64, void*);
    SymInitializeFn sym_init =
        (SymInitializeFn)(void*)GetProcAddress(dbgh, "SymInitialize");
    SymFromAddrFn sym_from =
        (SymFromAddrFn)(void*)GetProcAddress(dbgh, "SymFromAddr");
    HMODULE k32 = GetModuleHandleA("kernel32.dll");
    typedef USHORT(WINAPI* CaptureFn)(ULONG, ULONG, void**, PULONG);
    CaptureFn capture = k32
        ? (CaptureFn)(void*)GetProcAddress(k32, "RtlCaptureStackBackTrace")
        : NULL;
    if (!sym_init || !sym_from || !capture) {
        fprintf(stderr, "[btrace] %s: missing exports\n", tag);
        return;
    }
    HANDLE proc = GetCurrentProcess();
    sym_init(proc, NULL, TRUE);
    void* frames[32];
    ULONG captured = 0;
    capture(0, 32, frames, &captured);
    fprintf(stderr, "[btrace] %s: frames=%lu\n", tag, (unsigned long)captured);
    for (ULONG i = 0; i < captured; i++) {
        typedef struct { unsigned long SizeOfStruct; unsigned long TypeIndex;
                         unsigned long long Reserved[2]; unsigned long Index;
                         unsigned long Size; unsigned long long ModBase;
                         unsigned long Flags; unsigned long long Value;
                         unsigned long long Address; unsigned long Register;
                         unsigned long long Scope; unsigned long long Tag;
                         unsigned long NameLen; unsigned long MaxNameLen;
                         char Name[1]; } SymInfo;
        char buf[sizeof(SymInfo) + 160];
        SymInfo* info = (SymInfo*)buf;
        info->SizeOfStruct = sizeof(SymInfo);
        info->MaxNameLen = 159;
        DWORD64 disp = 0;
        if (sym_from(proc, (DWORD64)(uintptr_t)frames[i], &disp, info)) {
            fprintf(stderr, "[btrace] %s #%lu %p %s+0x%llx\n", tag,
                    (unsigned long)i, frames[i], info->Name,
                    (unsigned long long)disp);
        } else {
            fprintf(stderr, "[btrace] %s #%lu %p\n", tag, (unsigned long)i,
                    frames[i]);
        }
    }
    fflush(stderr);
}

/* 跨线程取栈：SuspendThread → GetThreadContext → StackWalk64 → SymFromAddr。
 * 同步原语全部 GetProcAddress 运行时加载（无链接依赖）。 */
void rt_diag_thread_stack(unsigned long tid, const char* tag) {
    HMODULE dbgh = LoadLibraryA("dbghelp.dll");
    HMODULE k32 = GetModuleHandleA("kernel32.dll");
    if (!dbgh || !k32) return;
    typedef BOOL(WINAPI* SymInitializeFn)(HANDLE, PCSTR, BOOL);
    typedef BOOL(WINAPI* SymFromAddrFn)(HANDLE, DWORD64, PDWORD64, void*);
    typedef BOOL(WINAPI* StackWalkFn)(DWORD, HANDLE, HANDLE, void*, void*,
                                      void*, void*, void*, void*);
    typedef unsigned long long(WINAPI* ModBaseFn)(HANDLE, DWORD64);
    typedef void*(WINAPI* FnTabFn)(HANDLE, DWORD64);
    typedef void*(WINAPI* OpenThreadFn)(DWORD, BOOL, DWORD);
    typedef DWORD(WINAPI* SuspendResumeFn)(HANDLE);
    typedef BOOL(WINAPI* GetCtxFn)(HANDLE, void*);
    SymInitializeFn sym_init = (SymInitializeFn)(void*)GetProcAddress(dbgh, "SymInitialize");
    SymFromAddrFn sym_from = (SymFromAddrFn)(void*)GetProcAddress(dbgh, "SymFromAddr");
    StackWalkFn stack_walk = (StackWalkFn)(void*)GetProcAddress(dbgh, "StackWalk64");
    ModBaseFn mod_base = (ModBaseFn)(void*)GetProcAddress(dbgh, "SymGetModuleBase64");
    FnTabFn fn_table = (FnTabFn)(void*)GetProcAddress(dbgh, "SymFunctionTableAccess64");
    OpenThreadFn open_thread = (OpenThreadFn)(void*)GetProcAddress(k32, "OpenThread");
    SuspendResumeFn suspend = (SuspendResumeFn)(void*)GetProcAddress(k32, "SuspendThread");
    SuspendResumeFn resume = (SuspendResumeFn)(void*)GetProcAddress(k32, "ResumeThread");
    GetCtxFn get_ctx = (GetCtxFn)(void*)GetProcAddress(k32, "GetThreadContext");
    if (!sym_init || !sym_from || !stack_walk || !mod_base || !fn_table ||
        !open_thread || !suspend || !resume || !get_ctx) {
        fprintf(stderr, "[tstack] %s: missing exports\n", tag);
        return;
    }
    HANDLE proc = GetCurrentProcess();
    sym_init(proc, NULL, TRUE);
    HANDLE th = open_thread(0x1FFFFF /* THREAD_ALL_ACCESS */, FALSE, tid);
    if (!th) {
        fprintf(stderr, "[tstack] %s tid=%lu: open failed\n", tag, tid);
        return;
    }
    if (suspend(th) == (DWORD)-1) {
        CloseHandle(th);
        return;
    }
    CONTEXT ctx;
    memset(&ctx, 0, sizeof(ctx));
    ctx.ContextFlags = CONTEXT_FULL;
    BOOL ok = get_ctx(th, &ctx);
    if (ok) {
        typedef struct { unsigned long SizeOfStruct; unsigned long TypeIndex;
                         unsigned long long Reserved[2]; unsigned long Index;
                         unsigned long Size; unsigned long long ModBase;
                         unsigned long Flags; unsigned long long Value;
                         unsigned long long Address; unsigned long Register;
                         unsigned long long Scope; unsigned long long Tag;
                         unsigned long NameLen; unsigned long MaxNameLen;
                         char Name[1]; } SymInfo;
        fprintf(stderr, "[tstack] %s tid=%lu begin\n", tag, tid);
        DWORD64 addr = (DWORD64)ctx.Rip;
        for (int i = 0; i < 48 && addr != 0; i++) {
            SymInfo info;
            memset(&info, 0, sizeof(info));
            char buf[sizeof(SymInfo) + 160];
            SymInfo* si = (SymInfo*)buf;
            si->SizeOfStruct = sizeof(SymInfo);
            si->MaxNameLen = 159;
            DWORD64 disp = 0;
            if (sym_from(proc, addr, &disp, si)) {
                fprintf(stderr, "[tstack] %s #%d %p %s+0x%llx\n", tag, i,
                        (void*)(uintptr_t)addr, si->Name,
                        (unsigned long long)disp);
            } else {
                fprintf(stderr, "[tstack] %s #%d %p\n", tag, i,
                        (void*)(uintptr_t)addr);
            }
            /* 帧回溯：RSP 扫描栈上的返回地址（StackWalk64 需完整回调集，
             * 此处用栈扫描近似：Rip/Rsp 起点向上扫，取模块内指针）。
             * 简化实现：仅符号化 Rip 与 Rsp 处栈内存中的模块内地址。 */
            if (i == 0) {
                /* 首帧后转栈扫描：Rsp 起连续 16KB 内的模块内地址。
                 * sym_from 成功打符号名（PDB 在场时）；失败打 mod+RVA
                 * （离线 llvm-symbolizer/导出表对照兜底）——两路都不失明。 */
                DWORD64 rsp = (DWORD64)ctx.Rsp;
                int printed = 0;
                for (DWORD64 a = rsp; a < rsp + 16384 && printed < 32; a += 8) {
                    DWORD64 v = *(DWORD64*)a;
                    if (v > 0x10000) {
                        DWORD64 mb = mod_base(proc, v);
                        if (mb != 0) {
                            SymInfo s2;
                            memset(&s2, 0, sizeof(s2));
                            char b2[sizeof(SymInfo) + 160];
                            SymInfo* si2 = (SymInfo*)b2;
                            si2->SizeOfStruct = sizeof(SymInfo);
                            si2->MaxNameLen = 159;
                            DWORD64 d2 = 0;
                            if (sym_from(proc, v, &d2, si2)) {
                                fprintf(stderr, "[tstack] %s stk rsp+%llu %s+0x%llx\n",
                                        tag, (unsigned long long)(a - rsp),
                                        si2->Name, (unsigned long long)d2);
                            } else {
                                fprintf(stderr,
                                        "[tstack] %s stk rsp+%llu %p mod=%llx rva=%llx\n",
                                        tag, (unsigned long long)(a - rsp),
                                        (void*)(uintptr_t)v,
                                        (unsigned long long)mb,
                                        (unsigned long long)(v - mb));
                            }
                            printed++;
                        }
                    }
                }
                break;
            }
        }
    }
    resume(th);
    CloseHandle(th);
    fflush(stderr);
}

/* 卡死线程取证（临时）：worker 执行 work 期间记录起始时刻与 fn 指针；
 * 转储打印 elapsed 超阈值者——识别「卡在 poll_inner / Monitor」的 worker。 */
#define RT_DIAG_MAX_WORKERS 64
static _Atomic(int64_t) g_diag_busy_since[RT_DIAG_MAX_WORKERS];
static _Atomic(uintptr_t) g_diag_busy_fn[RT_DIAG_MAX_WORKERS];

/* 低频诊断转储：worker 心跳自醒时由唯一认领者每 2s 打一行计数快照。
 * 挂死时计数冻结，watchdog kill 前 stderr 末行为最终现场。 */
static void rt_diag_maybe_dump(void) {
    static _Atomic(uint64_t) last_epoch;
    uint64_t cur = (uint64_t)(rt_tp_now_ms() / 2000);
    uint64_t prev = atomic_load_explicit(&last_epoch, memory_order_relaxed);
    if (cur > prev &&
        atomic_compare_exchange_strong_explicit(&last_epoch, &prev, cur,
                                                memory_order_relaxed,
                                                memory_order_relaxed)) {
        fprintf(stderr,
                "[diag] wake=%llu drop_el=%llu drop_data=%llu coro=%llu "
                "pollwork=%llu ipush=%llu ipop=%llu park=%llu hb=%llu "
                "reglate=%llu addprop=%llu\n",
                (unsigned long long)atomic_load_explicit(&g_diag_wake_calls, memory_order_relaxed),
                (unsigned long long)atomic_load_explicit(&g_diag_wake_drop_el, memory_order_relaxed),
                (unsigned long long)atomic_load_explicit(&g_diag_wake_drop_data, memory_order_relaxed),
                (unsigned long long)atomic_load_explicit(&g_diag_coro_wake, memory_order_relaxed),
                (unsigned long long)atomic_load_explicit(&g_diag_poll_work, memory_order_relaxed),
                (unsigned long long)atomic_load_explicit(&g_diag_inject_push, memory_order_relaxed),
                (unsigned long long)atomic_load_explicit(&g_diag_inject_pop, memory_order_relaxed),
                (unsigned long long)atomic_load_explicit(&g_diag_park_waits, memory_order_relaxed),
                (unsigned long long)atomic_load_explicit(&g_diag_hb_wakes, memory_order_relaxed),
                (unsigned long long)atomic_load_explicit(&g_diag_reg_late, memory_order_relaxed),
                (unsigned long long)atomic_load_explicit(&g_diag_add_prop, memory_order_relaxed));
        unsigned long long cen[8];
        rt_diag_task_census(cen);
        fprintf(stderr,
                "[census] live=%llu lc=%lld pool=%d p_bit_wk=%llu p_bit_nowk=%llu "
                "p_nobit_wk=%llu p_nobit_nowk_sm=%llu p_shell=%llu "
                "terminal=%llu polling=%llu\n",
                cen[0], (long long)rt_diag_live_count(), rt_diag_pool_count(),
                cen[1], cen[2], cen[3], cen[4], cen[5], cen[6], cen[7]);
        /* 卡死 worker 取证：打印 busy 超过 1s 的 worker（fn 指针识别卡点） */
        int64_t now_ms = rt_tp_now_ms();
        for (int w = 0; w < RT_DIAG_MAX_WORKERS; w++) {
            int64_t since = atomic_load_explicit(&g_diag_busy_since[w],
                                                 memory_order_relaxed);
            if (since != 0 && now_ms - since > 1000) {
                unsigned long long pst[4];
                rt_diag_poll_state(pst);
                fprintf(stderr, "[stuck] w=%d busy_ms=%lld fn=%p "
                                "poll_phase=%llu poll_task=%p reg_if=%llu reg_inner=%p\n",
                        w,
                        (long long)(now_ms - since),
                        (void*)atomic_load_explicit(&g_diag_busy_fn[w],
                                                    memory_order_relaxed),
                        pst[0], (void*)(uintptr_t)pst[1], pst[2],
                        (void*)(uintptr_t)pst[3]);
            }
        }
        /* el 驱动线程心跳（tick 冻结 = el 线程卡死）+ 全局 poll 相位 */
        unsigned long long pst[4];
        rt_diag_poll_state(pst);
        fprintf(stderr, "[el] ticks=%llu pending=%u ready=%u "
                        "poll_phase=%llu poll_task=%p reg_if=%llu reg_inner=%p "
                        "el_phase=%u reactor=%d\n",
                (unsigned long long)atomic_load_explicit(&g_diag_el_ticks, memory_order_relaxed),
                atomic_load_explicit(&g_diag_el_pending, memory_order_relaxed),
                atomic_load_explicit(&g_diag_el_ready, memory_order_relaxed),
                pst[0], (void*)(uintptr_t)pst[1], pst[2], (void*)(uintptr_t)pst[3],
                atomic_load_explicit(&g_diag_el_phase, memory_order_relaxed),
                g_diag_el_has_reactor);
        /* el 轮速率：正常 ≥20 tick/s（50ms 等待预算）；若 tks/s 崩到 <1，
         * 说明 el 每轮 tick/fire 耗时秒级（回调链卡死/追赶风暴），
         * 与 tick 内静态等待函数栈（0x15316/0x15b29）互证。 */
        {
            static int64_t diag_last_ms = 0;
            static unsigned long long diag_last_ticks = 0;
            int64_t now_t = rt_tp_now_ms();
            if (diag_last_ms != 0) {
                int64_t dt = now_t - diag_last_ms;
                unsigned long long dticks =
                    atomic_load_explicit(&g_diag_el_ticks, memory_order_relaxed)
                    - diag_last_ticks;
                fprintf(stderr, "[el-rate] dt=%lldms tks=%llu (%lld tks/s)\n",
                        (long long)dt, dticks,
                        dt > 0 ? (long long)((dticks * 1000) / dt) : -1);
            }
            diag_last_ms = now_t;
            diag_last_ticks =
                atomic_load_explicit(&g_diag_el_ticks, memory_order_relaxed);
        }
        fprintf(stderr, "[live] poll_entries=%llu repoll_iters=%llu "
                        "complete_no_waker=%llu\n",
                (unsigned long long)atomic_load_explicit(&g_diag_poll_entries, memory_order_relaxed),
                (unsigned long long)atomic_load_explicit(&g_diag_repoll_iters, memory_order_relaxed),
                (unsigned long long)atomic_load_explicit(&g_diag_complete_no_waker, memory_order_relaxed));
        fprintf(stderr, "[elmx] owner_tid=%ld waiters=%ld\n",
                atomic_load_explicit(&g_diag_el_mutex_owner, memory_order_relaxed),
                atomic_load_explicit(&g_diag_el_mutex_waiters, memory_order_relaxed));
        rt_diag_poll_own_dump();
        rt_diag_ring_dump();
        /* el 线程 + 全部 POLLING 持有者跨线程取栈（直接看卡点） */
        long el_tid = atomic_load_explicit(&g_diag_el_tid, memory_order_relaxed);
        if (el_tid) {
            rt_diag_thread_stack((unsigned long)el_tid, "el");
        }
        /* Monitor 死锁图：等待者 + 持锁者 */
        rt_mon_diag_dump();
    }
}

typedef struct {
#ifdef _WIN32
    CRITICAL_SECTION mutex;
    CONDITION_VARIABLE cond;
#else
    pthread_mutex_t mutex;
    pthread_cond_t  cond;
#endif
} rt_sync_t;

static void rt_sync_init(rt_sync_t* s) {
#ifdef _WIN32
    InitializeCriticalSection(&s->mutex);
    InitializeConditionVariable(&s->cond);
#else
    pthread_mutex_init(&s->mutex, NULL);
    pthread_cond_init(&s->cond, NULL);
#endif
}

static void rt_sync_destroy(rt_sync_t* s) {
#ifdef _WIN32
    DeleteCriticalSection(&s->mutex);
#else
    pthread_mutex_destroy(&s->mutex);
    pthread_cond_destroy(&s->cond);
#endif
}

static void rt_sync_lock(rt_sync_t* s) {
#ifdef _WIN32
    EnterCriticalSection(&s->mutex);
#else
    pthread_mutex_lock(&s->mutex);
#endif
}

static void rt_sync_unlock(rt_sync_t* s) {
#ifdef _WIN32
    LeaveCriticalSection(&s->mutex);
#else
    pthread_mutex_unlock(&s->mutex);
#endif
}

static void rt_sync_signal(rt_sync_t* s) {
#ifdef _WIN32
    WakeConditionVariable(&s->cond);
#else
    pthread_cond_signal(&s->cond);
#endif
}

static void rt_sync_broadcast(rt_sync_t* s) {
#ifdef _WIN32
    WakeAllConditionVariable(&s->cond);
#else
    pthread_cond_broadcast(&s->cond);
#endif
}

static void rt_sync_wait_timeout(rt_sync_t* s, uint32_t timeout_ms) {
#ifdef _WIN32
    SleepConditionVariableCS(&s->cond, &s->mutex, (DWORD)timeout_ms);
#else
    struct timespec ts;
    clock_gettime(CLOCK_REALTIME, &ts);
    ts.tv_sec  += (time_t)(timeout_ms / 1000);
    ts.tv_nsec += (long)((timeout_ms % 1000) * 1000000);
    if (ts.tv_nsec >= 1000000000) { ts.tv_sec++; ts.tv_nsec -= 1000000000; }
    pthread_cond_timedwait(&s->cond, &s->mutex, &ts);
#endif
}

/* ---- 线程抽象 ---- */

typedef struct {
    rt_thread_t handle;
    int32_t     id;
} rt_worker_thread;

#if defined(_WIN32)
static unsigned __stdcall worker_main(void* arg);
#else
static void* worker_main(void* arg);
#endif

static int rt_tp_thread_create(rt_thread_t* t, void* arg) {
#ifdef _WIN32
    /* CreateThread + CRT 在进程退出期可致 flaky 0xC0000005（H1 UnitTest）。 */
    uintptr_t h = _beginthreadex(NULL, 0, worker_main, arg, 0, NULL);
    if (!h) return -1;
    *t = (HANDLE)h;
    return 0;
#else
    return pthread_create(t, NULL, worker_main, arg);
#endif
}

static void rt_tp_thread_join(rt_thread_t t) {
#ifdef _WIN32
    WaitForSingleObject(t, INFINITE);
    CloseHandle(t);
#else
    pthread_join(t, NULL);
#endif
}

/* ---- 数据结构 ---- */

/* rt_work_node 定义已移至 rt_abi.h（RFC 009 §3：消除 rt_rw_pool，统一 node 类型）。
 * 原 rt_threadpool.c 内部定义（work + next）扩展为含 task/action_fn/action_data/ct
 * 扩展字段，rt_task_run 直接从 work_pool 分配，消除 rt_rw_pool CAS 开销。 */

/* work-node 池（RFC 032 A5② 防守性优化 · ws 派发预算）：free-list 为 lock-free
 * Treiber stack + tag 防 ABA（见下方实现前的详细设计说明）。 */
struct rt_work_pool {
    _Atomic(uint64_t) free_head;  /* (tag << N) | ptr；0 = 空 */
};

/* ---- 全局无锁 MPMC injector（RFC 009 §6 · 对齐 tokio Injector / Rayon inject / Go global runq）----
 *
 * 原理（2026-08-07 用户要求从原理分析 + 参照 Go/Rust 新兴语言方案）：
 *   Go scheduler、tokio 多线程 runtime、Rayon 对「外部 spawn」一律推入**全局**
 *   lock-free MPMC 队列，而非投递给某个 worker 的私有队列。原因：
 *     1. spawner 只写 injector 的 enqueue_pos 单条热 cache line——无向 worker 私有
 *        lifo_slot 的 RFO 乒乓（原 round-robin 每 spawn 一次 atomic_exchange 把 slot
 *        cache line 从 worker CPU 抢到 spawner CPU 再抢回，~80-160ns/op，是本机
 *        无法超越 .NET 的结构主因之一）；
 *     2. worker 可一次 drain 多条任务摊销 CAS（tokio/Go 的批处理语义）；
 *     3. 任意空闲 worker 都能取（天然负载均衡，无随机投递到繁忙 worker 的滞留）。
 *
 * 本实现为 Vyukov 有界 MPMC 队列（业界 lock-free queue 基准参照）：
 *   - 定长槽位数组 + 单调递增 enqueue_pos/dequeue_pos + 槽位序号（ABA 防护）；
 *   - push：fetch_add enqueue_pos → CAS 槽位序号 claim → 写 data → release 发布；
 *   - pop：gate(dequeue_pos<enqueue_pos 非空) → 槽位就绪(seq==pos+1) → CAS 推进
 *     dequeue_pos 独占位置 → 读 data → 递增 cycle（非阻塞空返回 NULL，不消耗位置）；
 *   - 有界：CAP 满时 push 自旋等待槽位（12 消费端排空极快，正常不触发）。 */
#define RT_INJECT_CAP_SHIFT 14
#define RT_INJECT_CAP   (1u << RT_INJECT_CAP_SHIFT)   /* 16384 槽位 ≈ 256KB/池 */
#define RT_INJECT_MASK  (RT_INJECT_CAP - 1u)

typedef struct rt_inject_slot {
    _Atomic(uint64_t) seq;   /* 槽位序号：enqueue 发布用 pos+1；dequeue 用 pos+CAP+1 */
    _Atomic(void*)     data; /* rt_work_node* */
} rt_inject_slot;

typedef struct rt_injector {
    rt_inject_slot*  buffer;         /* 只读指针，init 后不变 */
    _Atomic(uint64_t) enqueue_pos;   /* 生产者独占写（单调递增） */
    char _pad[48];                   /* 64B 对齐：enqueue_pos 与 dequeue_pos 分 cache line
                                      *（消除生产者写 enqueue_pos RFO 使消费者 dequeue_pos
                                      * 线失效的 false-sharing；Vyukov MPMC 原形即此布局） */
    _Atomic(uint64_t) dequeue_pos;   /* 消费者争用（12 worker CAS 独占位置） */
} rt_injector;

/* per-worker 上下文（TLS）。
 * RFC 009 M1 扩展：新增 lifo_slot + is_parked 字段，实现 LIFO slot 优化。
 * lifo_slot 为原子单 slot（atomic_exchange 无锁），不可偷——
 *   - owner push（spawn_local）时若 slot 已占用则溢出到本地 deque（rt_ws_push）
 *   - 非 owner push（spawn 快路径投递给 parked worker）时 slot 已占用则溢出到全局队列
 *     （P0 修复：保持 Chase-Lev「仅 owner push」不变式，见 rt_worker_push_lifo）
 *   - pop 时优先 atomic_exchange NULL 取出，未命中再走 deque
 * is_parked 用于 needs_wakeup 快路径：worker park 前标记，外部 broadcast 后清除。
 *
 * RFC 009（2026-08-07）：is_parked 升级为三态状态机（参照 tokio notify）：
 *   RT_WORKER_RUNNING(0) / RT_WORKER_PARKED(1) / RT_WORKER_NOTIFIED(2)
 * spawner CAS PARKED→NOTIFIED 成功才 signal，消除 burst spawn 的 redundant signal。
 *
 * 注意：结构定义已移至 rt_abi.h 作为完整类型定义
 *（rt_preempt.c 等独立翻译单元需访问 preempt 字段），
 * rt_threadpool.c 无需重复定义。 */

/* is_parked 三态（RFC 009 · 参照 tokio notify 状态机）。
 * 转换图：
 *   RUNNING --(worker park)--> PARKED --(spawner notify)--> NOTIFIED --(worker wake)--> RUNNING
 *   RUNNING --(spawner notify 提前)--> NOTIFIED --(worker park 尝试)--> RUNNING（消费 notification，不 park）
 * parked_count 统计「非 RUNNING worker」（PARKED + NOTIFIED），spawner 据此决定是否扫描。 */
#define RT_WORKER_RUNNING   0
#define RT_WORKER_PARKED    1
#define RT_WORKER_NOTIFIED  2

/* ThreadPool 主结构 */
struct rt_threadpool {
    int32_t           n_workers;
    rt_worker_thread* workers;
    rt_worker_ctx*    worker_ctxs;     /* per-worker 上下文数组 */
    rt_ws_deque**     deques;          /* per-worker deque 数组（供 steal 索引） */

    /* 全局 injector queue（外部 spawn 入口；节点 = rt_work_node，复用池） */
    rt_work_node*     global_head;
    rt_work_node*     global_tail;
    _Atomic(int32_t)  global_size;   /* 有全局 work（>0 时 try_get_work 才取锁） */

    /* 同步 */
    rt_sync_t         sync;            /* 保护 global queue + park/wake */
    _Atomic(int32_t)  pending_count;   /* 未完成 work 总数（含 global + local） */
    _Atomic(int32_t)  parked_count;    /* 当前 parked worker 数（spawn 快路径跳过扫描的提示） */
    _Atomic(int32_t)  shutdown;        /* 1=正在关闭 */
    struct rt_work_pool work_pool;     /* A5② work-node 复用池（RFC 032） */
    rt_injector          injector;     /* RFC 009 §6: 全局无锁 MPMC injector（外部 spawn 入口） */

    /* TLS key（POSIX pthread_key_t / Windows FlsCallback） */
#ifdef _WIN32
    DWORD             tls_index;
#else
    pthread_key_t     tls_key;
#endif
};

/* ---- work-node 池（RFC 032 A5② 防守性优化 · ws 派发预算）----
 * 每次 spawn 都 malloc 一个节点，而 H1 纪律（关闭期勿 free）使节点从不释放 →
 * 长时间 spawn 的堆持续增长，缺页/commit 成本累积，ws 派发延迟漂移
 * （安静基线 ~240ns vs 重负载 ~400ns）。回池复用消除该漂移源。
 * free-list 为 lock-free Treiber stack + tag 防 ABA：
 *   - 64 位：指针低 48 位 + tag 高 16 位（Windows/Linux 用户态地址 < 2^48）
 *   - 32 位：指针低 32 位 + tag 高 32 位
 *   - work_node_alloc：pop 成功即复用；池空回落 malloc
 *   - 消费端（worker pop / 全局队列 pop）整节点回池
 *   - 池本身在 destroy 后按 H1 漏至进程退出（无迟到访问，与池结构同策） */
#if UINTPTR_MAX > 0xFFFFFFFFu
#  define POOL_PTR_MASK 0x0000FFFFFFFFFFFFull
#  define POOL_TAG_MASK 0xFFFF000000000000ull
#  define POOL_TAG_ONE  0x0001000000000000ull
#else
#  define POOL_PTR_MASK 0xFFFFFFFFull
#  define POOL_TAG_MASK 0xFFFFFFFF00000000ull
#  define POOL_TAG_ONE  0x100000000ull
#endif

static rt_work_node* work_pool_pop(rt_threadpool* pool) {
    _Atomic(uint64_t)* head = &pool->work_pool.free_head;
    uint64_t old = atomic_load_explicit(head, memory_order_relaxed);
    for (;;) {
        rt_work_node* node = (rt_work_node*)(uintptr_t)(old & POOL_PTR_MASK);
        if (!node) return NULL;
        rt_work_node* next =
            atomic_load_explicit(&node->next, memory_order_acquire);
        uint64_t upd = ((old + POOL_TAG_ONE) & POOL_TAG_MASK)
                     | ((uintptr_t)next & POOL_PTR_MASK);
        if (atomic_compare_exchange_weak_explicit(head, &old, upd,
                memory_order_acquire, memory_order_relaxed)) {
            return node;
        }
    }
}

static void work_pool_push(rt_threadpool* pool, rt_work_node* node) {
    _Atomic(uint64_t)* head = &pool->work_pool.free_head;
    uint64_t old = atomic_load_explicit(head, memory_order_relaxed);
    for (;;) {
        atomic_store_explicit(&node->next,
                              (rt_work_node*)(uintptr_t)(old & POOL_PTR_MASK),
                              memory_order_release);
        uint64_t upd = (old & POOL_TAG_MASK)
                     | ((uintptr_t)node & POOL_PTR_MASK);
        if (atomic_compare_exchange_weak_explicit(head, &old, upd,
                memory_order_release, memory_order_relaxed)) {
            return;
        }
    }
}

/* ---- per-thread work-node 批量缓存（RFC 009 · 对齐 .NET 每线程分配上下文 / Go per-P）----
 *
 * 问题（原理分析 · 2026-08-07）：Task.Run 主线程 spawn 每任务都 work_pool_pop（对
 * pool->work_pool.free_head 的 CAS），而 12 个 worker 执行完每任务都 work_pool_push
 * （同一 free_head 的 CAS）——13 线程争用单条 cache line，是 spawn 串行路径（wall 瓶颈）
 * 的主要原子争用之一（task_spawn_wait ~476ns/op vs .NET ~245ns/op 的主因）。
 *
 * 方案（对齐 Go sched gFree per-P / .NET GC 每线程分配上下文）：每线程私有 LIFO 缓存。
 *   - alloc：一次从全局 work_pool 批量 pop N 个到私有缓存，之后 N 次 alloc 零原子；
 *   - 全局 free_head 争用从「每任务 1 CAS」降至「每 N 任务 1 CAS」。
 * 节点为 pool 无关内存，跨池复用安全（work_pool 仅 free-list）。缓存节点按 H1 纪律
 * 漏至进程退出（与 work_pool 同策）。 */
#define RT_NODE_CACHE_BATCH 32
#define RT_NODE_CACHE_MAX   256

#ifdef _WIN32
static __declspec(thread) rt_work_node* g_node_cache_head  = NULL;
static __declspec(thread) int32_t       g_node_cache_count = 0;
#else
static _Thread_local rt_work_node* g_node_cache_head  = NULL;
static _Thread_local int32_t       g_node_cache_count = 0;
#endif

static rt_work_node* node_cache_pop(void) {
    if (g_node_cache_count <= 0 || !g_node_cache_head) return NULL;
    rt_work_node* n = g_node_cache_head;
    g_node_cache_head = (rt_work_node*)(uintptr_t)atomic_load_explicit(&n->next, memory_order_relaxed);
    g_node_cache_count--;
    return n;
}

static rt_work_node* work_node_alloc(rt_threadpool* pool) {
    rt_work_node* n = node_cache_pop();
    if (n) {
        /* 终止 next：池节点复用 waker 槽位作 next 时残留的 pool-link 必须清空。
         * 单节点 spawn（injector/LIFO/deque）执行时 rt_run_work_list 据 node->next
         * 判链，残留 pool-link 会让 worker 越界 drain 到已入队/已释放节点（UAF）。
         * 批式路径 rt_tp_batch_append 会自行重设 next，不受影响。 */
        atomic_store_explicit(&n->next, NULL, memory_order_relaxed);
        return n;
    }
    /* 缓存空 → 批量 pop 填充私有缓存（1 次全局 CAS 换取 BATCH 个零原子 alloc） */
    for (int32_t i = 0; i < RT_NODE_CACHE_BATCH && g_node_cache_count < RT_NODE_CACHE_MAX; i++) {
        rt_work_node* m = work_pool_pop(pool);
        if (!m) break;
        atomic_store_explicit(&m->next, g_node_cache_head, memory_order_relaxed);
        g_node_cache_head = m;
        g_node_cache_count++;
    }
    n = node_cache_pop();
    if (n) {
        atomic_store_explicit(&n->next, NULL, memory_order_relaxed);
        return n;
    }
    n = (rt_work_node*)malloc(sizeof(rt_work_node));
    if (n) atomic_store_explicit(&n->next, NULL, memory_order_relaxed);
    return n;
}

/* worker 侧批量归还（RFC 009：free 侧同样摊销全局 free_head 争用）。
 * 节点先入本线程私有缓存，缓存满（RT_NODE_CACHE_MAX）时批量 push 回全局 work_pool
 * ——与 alloc 侧共享同一 TLS 缓存：缓存空→批量 pop，缓存满→批量 push，全局 free_head
 * 争用从「每任务 1 CAS」降至「每 MAX 任务 1 CAS」。 */
static void work_node_recycle_local(rt_threadpool* pool, rt_work_node* node) {
    if (!node) return;
    atomic_store_explicit(&node->next, g_node_cache_head, memory_order_relaxed);
    g_node_cache_head = node;
    g_node_cache_count++;
    if (g_node_cache_count >= RT_NODE_CACHE_MAX) {
        rt_work_node* n = g_node_cache_head;
        g_node_cache_head = NULL;
        g_node_cache_count = 0;
        while (n) {
            rt_work_node* next =
                (rt_work_node*)(uintptr_t)atomic_load_explicit(&n->next, memory_order_relaxed);
            work_pool_push(pool, n);
            n = next;
        }
    }
}

/* ---- 全局无锁 MPMC injector 实现（Vyukov bounded queue）---- */

static int rt_inject_init(rt_threadpool* pool) {
    rt_injector* q = &pool->injector;
    q->buffer = (rt_inject_slot*)calloc(RT_INJECT_CAP, sizeof(rt_inject_slot));
    if (!q->buffer) return -1;
    for (uint32_t i = 0; i < RT_INJECT_CAP; i++) {
        atomic_init(&q->buffer[i].seq, (uint64_t)i);
        atomic_init(&q->buffer[i].data, NULL);
    }
    atomic_init(&q->enqueue_pos, 0);
    atomic_init(&q->dequeue_pos, 0);
    return 0;
}

/* push 一个 rt_work_node* 到 injector。CAP 满时自旋等待消费端排空（背压）。
 * enqueue_pos 单调递增；每个 push 经 fetch_add 独占唯一位置 pos。
 *
 * 经典 Vyukov 无锁 MPMC（对齐 tokio Injector 语义）：
 *   - **无需 claim CAS**：fetch_add 已保证每个 producer 独占唯一 pos；
 *     同数组单元被循环复用，仅当单元回归「enqueue-ready」（seq==pos）时才可写。
 *   - 单元 seq 从 pos（空）→ pos+1（已发布）**单次 release 转换**：先写 data，
 *     再 release store seq=pos+1，保证 consumer 读到 seq==pos+1 时 data 必可见。
 *     原实现加 claim CAS（seq pos→pos+1）会在 data 写入前就把 seq==pos+1 暴露给
 *     consumer → consumer 读未写 data → 陈旧指针 → 严重数据竞争。 */
static void rt_inject_push(rt_threadpool* pool, rt_work_node* node) {
    rt_injector* q = &pool->injector;
    uint64_t pos = atomic_fetch_add_explicit(&q->enqueue_pos, 1, memory_order_relaxed);
    rt_inject_slot* slot = &q->buffer[pos & RT_INJECT_MASK];
    long diag_spins = 0;
    for (;;) {
        /* readiness 自旋 load 用 relaxed：仅检测槽位是否回归「空」（前一轮已回收），
         * 真实发布在下方 release store seq。relaxed 省 spin 轮 acquire 栅栏。 */
        uint64_t seq = atomic_load_explicit(&slot->seq, memory_order_relaxed);
        if ((int64_t)(seq - pos) == 0) break;   /* 单元就绪：无前一轮残留 */
        /* 单元仍持有前一轮 data（未被 dequeue，队列满背压）或前序 producer 未发布；
         * 12 消费端排空极快，正常不触发；不会死锁（满队列必有多消费端在 drain）。 */
        if (++diag_spins == 200000000L) {
            rt_diag_btrace("inject");
        }
        RT_CPU_RELAX();
    }
    atomic_store_explicit(&slot->data, node, memory_order_relaxed);
    atomic_store_explicit(&slot->seq, pos + 1, memory_order_release);  /* 单次发布 data */
    atomic_fetch_add_explicit(&g_diag_inject_push, 1, memory_order_relaxed);
}

/* ---- 全局无锁 MPMC injector 单 pop（RFC 009 §6 · 对齐 tokio Injector / Go global runq）----
 *
 * 单 pop（Vyukov 有界队列 dequeue）：
 *   - gate：dequeue_pos < enqueue_pos 才可能非空；
 *   - 槽位就绪（seq == pos+1）后 CAS 推进 dequeue_pos 独占位置 → 读 data → 回收槽位
 *     （seq = pos+CAP，供 pos+CAP 处下一次 enqueue）。
 *   - 非阻塞空检测：pos >= eq 直接返回 0，不消耗位置。
 *
 * 说明（2026-08-07 实测）：曾尝试「一次 CAS 批量 drain BATCH 个槽位 + 余量 backfill
 * 到本地 deque」（tokio/Go 批处理语义）以降低 dequeue_pos 争用。实测该方案在本机
 * 为净回归（task_spawn_wait min 430ns/op → 1939ns/op，4.5×）：backfill 使每任务
 * 多一次 rt_ws_push + rt_ws_pop（各含内存栅栏），且单 spawner 场景 injector 稀疏时
 * 每 pop 都要扫描至多 BATCH 个槽位。单 pop 无该往返开销，是当前正确且更快的形态。 */
static int32_t rt_inject_pop(rt_threadpool* pool, rt_work_node** out) {
    rt_injector* q = &pool->injector;
    for (;;) {
        /* **下钻优化（2026-08-07）**：dequeue_pos/enqueue_pos 空检测 load 用 relaxed。
         * 二者仅是「可能非空」启发式 + 算槽位索引；真实同步点在 slot->seq acquire
         * 与下方 dequeue_pos CAS（acq_rel）。stale 读 pos 仅使 CAS 用旧值失败重读，
         * stale 读 eq 仅使空检测不准（多一次 steal/park 双查兜底）——均不破坏正确性。
         * relaxed 省去 acquire 栅栏（x86 下 load 多为普通 mov）。 */
        uint64_t pos = atomic_load_explicit(&q->dequeue_pos, memory_order_relaxed);
        uint64_t eq = atomic_load_explicit(&q->enqueue_pos, memory_order_relaxed);
        if (pos >= eq) return 0;  /* 空：不消耗位置（非阻塞空检测） */
        rt_inject_slot* slot = &q->buffer[pos & RT_INJECT_MASK];
        if (atomic_load_explicit(&slot->seq, memory_order_acquire) != pos + 1) {
            /* 槽位未就绪：producer 正在推进中，退避等待（必最终发布） */
            RT_CPU_RELAX(); continue;
        }
        /* CAS 独占 [pos, pos+1)：仅成功者消费该位置，失败者重读 */
        if (atomic_compare_exchange_weak_explicit(&q->dequeue_pos, &pos, pos + 1,
                memory_order_acq_rel, memory_order_relaxed)) {
            *out = (rt_work_node*)atomic_load_explicit(&slot->data, memory_order_relaxed);
            /* 回收槽位：下次 enqueue 于位置 pos+CAP，等待 seq==pos+CAP */
            atomic_store_explicit(&slot->seq, pos + RT_INJECT_CAP, memory_order_release);
            atomic_fetch_add_explicit(&g_diag_inject_pop, 1, memory_order_relaxed);
            return 1;
        }
        /* CAS 失败：其他消费者抢先推进 → 重读 */
    }
}


/* ---- TLS 工具 ---- */

#ifdef _WIN32
static __declspec(thread) rt_worker_ctx* g_tls_worker_ctx = NULL;
#else
static _Thread_local rt_worker_ctx* g_tls_worker_ctx = NULL;
#endif

static void rt_pool_set_worker_ctx(rt_threadpool* pool, rt_worker_ctx* ctx) {
#ifdef _WIN32
    TlsSetValue(pool->tls_index, ctx);
#else
    pthread_setspecific(pool->tls_key, ctx);
#endif
    /* 真 TLS：不依赖「全局 current pool」，多池并存时 worker_id/current 仍正确。 */
    g_tls_worker_ctx = ctx;
}

static rt_worker_ctx* rt_pool_get_worker_ctx(rt_threadpool* pool) {
#ifdef _WIN32
    return (rt_worker_ctx*)TlsGetValue(pool->tls_index);
#else
    return (rt_worker_ctx*)pthread_getspecific(pool->tls_key);
#endif
}

static void rt_pool_init_tls(rt_threadpool* pool) {
#ifdef _WIN32
    pool->tls_index = TlsAlloc();
#else
    pthread_key_create(&pool->tls_key, NULL);
#endif
}

static void rt_pool_destroy_tls(rt_threadpool* pool) {
#ifdef _WIN32
    TlsFree(pool->tls_index);
#else
    pthread_key_delete(pool->tls_key);
#endif
}

/* ---- 全局 injector queue 操作（调用方持有 sync 锁） ---- */

static void global_queue_push_locked(rt_threadpool* pool, rt_work_node* node) {
    if (!node) return;  /* OOM，丢弃 work */
    atomic_store_explicit(&node->next, NULL, memory_order_release);
    if (pool->global_tail) {
        pool->global_tail->next = node;
    } else {
        pool->global_head = node;
    }
    pool->global_tail = node;
    atomic_fetch_add_explicit(&pool->global_size, 1, memory_order_relaxed);
}

static int global_queue_pop_locked(rt_threadpool* pool, rt_work_t* out, rt_work_node** out_node) {
    if (!pool->global_head) return 0;
    rt_work_node* node = pool->global_head;
    pool->global_head = atomic_load_explicit(&node->next, memory_order_acquire);
    if (!pool->global_head) pool->global_tail = NULL;
    /* 终止被弹出节点的 next：它仍指向新 global_head，若 rt_run_work_list 据此
     * drain 会越界消费仍排队（或正被其他 worker 弹出）的节点。持锁弹出，安全。 */
    atomic_store_explicit(&node->next, NULL, memory_order_relaxed);
    atomic_fetch_sub_explicit(&pool->global_size, 1, memory_order_relaxed);
    *out = node->work;
    if (out_node) *out_node = node;  /* 调用方在 work.fn 执行后回收（RFC 009 §3） */
    return 1;
}

/* ---- overflow handler（deque 已满时由 rt_ws_push 调用） ----
 *
 * H1：ctx 为**该 deque 所属池**（rt_ws_deque_set_overflow_ctx），禁止进程级
 * g_current_pool。多 ThreadPoolScheduler 并存时，后者 create 会覆盖全局
 * current → 前者 worker 溢出写入已 Destroy 的池 → 堆损坏，WriteResults
 * 末条（常为 Wiki_Snapshot_Restore）放大为 flaky 0xC0000005。 */

/* 唤醒任一 parked worker（per-worker condvar）。用于全局队列 push 后。
 * 调用方**不得**持有 pool->sync（避免「ps → pool->sync」（worker park 双查）与
 * 「pool->sync → ps」（若持锁唤醒）的 ABBA 死锁）。返回 1=唤醒了一个，0=无。
 *
 * RFC 009（2026-08-07）三态化：扫描 is_parked==PARKED（非 NOTIFIED），CAS
 * PARKED→NOTIFIED 成功才 signal——避免唤醒已 NOTIFIED 的 worker（redundant
 * signal）。NOTIFIED worker 已被前一次 spawn notify，即将醒来，无需重复 signal。 */
static int wake_one_parked(rt_threadpool* pool) {
    for (int32_t i = 0; i < pool->n_workers; i++) {
        rt_worker_ctx* ctx = &pool->worker_ctxs[i];
        int32_t expected = RT_WORKER_PARKED;
        if (atomic_compare_exchange_strong_explicit(&ctx->is_parked, &expected,
                RT_WORKER_NOTIFIED, memory_order_acq_rel, memory_order_relaxed)) {
            /* CAS 成功（PARKED→NOTIFIED）：worker 在 park，需 signal 唤醒 */
            rt_sync_t* ps = (rt_sync_t*)ctx->park_sync;
            if (ps) {
                rt_sync_lock(ps);
                rt_sync_signal(ps);
                rt_sync_unlock(ps);
            }
            return 1;
        }
        /* CAS 失败（RUNNING 或 NOTIFIED）：该 worker 无需唤醒，继续扫描下一个 */
    }
    return 0;
}

static void ws_overflow_handler(void* ctx, void* item) {
    /* item 是 rt_work_node*（A5②：LIFO/deque 溢出节点，整块入全局队列，无拷贝）。 */
    rt_work_node* node = (rt_work_node*)item;
    if (!node) return;
    rt_threadpool* pool = (rt_threadpool*)ctx;
    if (!pool) {
        /* ctx 已摘（destroy 窗口）：回退 TLS 所属池。 */
        rt_worker_ctx* w = g_tls_worker_ctx;
        pool = w ? (rt_threadpool*)w->pool : NULL;
    }
    if (!pool) {
        /* H1: destroy 窗口勿 free(node)——与 CRT/报告期分配交织可放大堆损伤；
         * 漏包装至进程退出（与 Task.Run trampoline 同策）。 */
        return;
    }
    rt_sync_lock(&pool->sync);
    global_queue_push_locked(pool, node);
    rt_sync_unlock(&pool->sync);
    /* 唤醒任一 parked worker 消费全局队列（per-worker cv；勿持 pool->sync 唤醒） */
    wake_one_parked(pool);
}

/* ---- 外部 spawn 生产者本地批（RFC 038 下钻 · 2026-08-07）----
 *
 * 问题（原理分析）：bench_task_spawn_wait 主线程串行 spawn 每任务一次 Vyukov
 * injector 槽位 cold-write（槽位 cache line 归消费者独占，跨核 RFO ~100-200ns），
 * 是 spawn 串行路径主成本（task_spawn_wait ~248ns/op 的大头），且 12 worker 对
 * dequeue_pos 的 CAS 争用同样随任务数线性放大。
 *
 * 方案（对齐 .NET ConcurrentQueue 分槽段 / Go per-P runq 本质）：生产者把最多
 * RT_TP_BATCH 个 rt_work_node 串成本线程独占链表（本地 cache line 写，快），再
 * **一次** injector push 发布链表头——槽位 cold-write 从 N 次降至 1 次，dequeue
 * CAS 争用同降 ~RT_TP_BATCH 倍。消费者 pop 链表头后整链 drain（见 rt_run_work_list）。
 *
 * 正确性关键：批可能滞留（尾批 < RT_TP_BATCH）。所有可能阻塞的等待入口——
 * rt_task_wait_all/wait_timeout/wait_ct/wait_any + rt_threadpool_wait_idle +
 * worker park——必须先冲刷当前线程批，保证「已 spawn 的任务必发布」，闭合
 * spawn-后-wait 的 lost-work。同线程 spawn+wait（本机所有真实用法）恒正确；
 * 跨线程 wait 由批满周期性冲刷兜底（尾批极窄，见 rt_threadpool_flush_local）。 */
#define RT_TP_BATCH 32

typedef struct rt_tp_batch {
    rt_threadpool* pool;
    rt_work_node*  head;
    rt_work_node*  tail;
    int32_t        count;
} rt_tp_batch;
#ifdef _WIN32
static __declspec(thread) rt_tp_batch g_tp_batch = { NULL, NULL, NULL, 0 };
#else
static _Thread_local rt_tp_batch g_tp_batch = { NULL, NULL, NULL, 0 };
#endif

/* 生产者批的线程退出冲刷（RFC 038 下钻 · fire-and-forget 发布保证）。
 * 批按「任何阻塞等待入口冲刷」闭合 spawn-后-wait；但对 fire-and-forget（spawn 后
 * 永不等待）的尾批，若线程直接退出则永不发布 → 任务静默丢失。故注册 TLS 析构：
 * 任意线程首次使用批时置 sentinel，线程退出时析构调用 flush_local 兜底发布。
 * pool 生命周期：进程级 default pool 存活至退出，析构时仍有效；自定义池在销毁前
 * 会先 join 其 worker（见 rt_threadpool_destroy），本线程批已在销毁路径冲刷。 */
static _Atomic(int32_t) g_tp_batch_key_ready = 0;
#ifdef _WIN32
static DWORD g_tp_batch_key = TLS_OUT_OF_INDEXES;
static void CALLBACK tp_batch_flush_dtor(void* p) {
    (void)p; rt_threadpool_flush_local();
}
#else
static pthread_key_t g_tp_batch_key;
static int           g_tp_batch_key_init = 0;
static void tp_batch_flush_dtor(void* p) {
    (void)p; rt_threadpool_flush_local();
}
#endif

/* 确保本线程注册批的线程退出析构（幂等；仅首次置 sentinel）。 */
static void rt_tp_batch_ensure_dtor(void) {
    if (atomic_load_explicit(&g_tp_batch_key_ready, memory_order_acquire)) {
#ifdef _WIN32
        if (!FlsGetValue(g_tp_batch_key)) FlsSetValue(g_tp_batch_key, (void*)(uintptr_t)1);
#else
        if (!pthread_getspecific(g_tp_batch_key)) pthread_setspecific(g_tp_batch_key, (void*)(uintptr_t)1);
#endif
        return;
    }
    /* 首次：注册 key 并发布 ready（并发首次仅首个成功者写入，其余线程各自注册
     * 会泄漏 key，但仅发生于多线程同时首次 spawn 的罕见启动窗口，可接受）。 */
#ifdef _WIN32
    if (g_tp_batch_key == TLS_OUT_OF_INDEXES) {
        DWORD k = FlsAlloc(tp_batch_flush_dtor);
        if (k == TLS_OUT_OF_INDEXES) return;
        g_tp_batch_key = k;
        atomic_store_explicit(&g_tp_batch_key_ready, 1, memory_order_release);
    }
    if (!FlsGetValue(g_tp_batch_key)) FlsSetValue(g_tp_batch_key, (void*)(uintptr_t)1);
#else
    if (!g_tp_batch_key_init) {
        if (pthread_key_create(&g_tp_batch_key, tp_batch_flush_dtor) != 0) return;
        g_tp_batch_key_init = 1;
        atomic_store_explicit(&g_tp_batch_key_ready, 1, memory_order_release);
    }
    if (!pthread_getspecific(g_tp_batch_key)) pthread_setspecific(g_tp_batch_key, (void*)(uintptr_t)1);
#endif
}

/* 冲刷当前线程的生产者批：把整条链表一次 injector push 发布，并计入 pending_count
 * + 唤醒 parked worker。任何阻塞等待前必须调用（见上方正确性说明）。
 * 顺序：**先计入 pending_count 再发布**（add-before-push）——任务对 worker 可见（可取走）
 * 时计数必已落账，wait_idle 的「排队未取」判定不漏（start-based，见 rt_run_work_list）。 */
void rt_threadpool_flush_local(void) {
    rt_tp_batch* b = &g_tp_batch;
    if (b->count <= 0) return;
    rt_threadpool* pool = b->pool;
    rt_work_node* head = b->head;
    int32_t n = b->count;
    b->head = b->tail = NULL;
    b->count = 0;
    b->pool = NULL;
    atomic_fetch_add_explicit(&pool->pending_count, n, memory_order_release);
    rt_inject_push(pool, head);
    if (atomic_load_explicit(&pool->parked_count, memory_order_acquire) > 0) {
        wake_one_parked(pool);
    }
}

/* 追加一个节点到当前线程批；批满即冲刷。 */
static void rt_tp_batch_append(rt_threadpool* pool, rt_work_node* node) {
    /* 首次使用批：注册线程退出析构（fire-and-forget 发布保证，见 dtor 说明）。 */
    rt_tp_batch_ensure_dtor();
    rt_tp_batch* b = &g_tp_batch;
    if (b->pool != pool) {
        if (b->count > 0) rt_threadpool_flush_local();
        b->pool = pool;
    }
    atomic_store_explicit(&node->next, NULL, memory_order_relaxed);
    if (b->tail) {
        atomic_store_explicit(&b->tail->next, node, memory_order_relaxed);
    } else {
        b->head = node;
    }
    b->tail = node;
    if (++b->count >= RT_TP_BATCH) rt_threadpool_flush_local();
}

/* ---- worker 主循环 ---- */

/* pending_count 语义（2026-08-13 定稿）：**start-based**——「排队未开始执行」的任务数。
 *
 * 此前为 completion-based 批量递减：每个任务**完成后**才累积到 pending_batch，每 16 次
 * 批量 fetch_sub（或 idle/park 时 flush）。缺陷：Task.Wait() 返回瞬间（任务 READY 刚落账）
 * 主线程读 PendingTaskCount 可能读到尚未 flush 的批 → 计数未归零
 * （examples/UnitTest ThreadPoolScheduler_Run_Action_Completes / _Pressure_Many_Tasks 实测
 * pending=1 / 8）。
 *
 * 新语义：worker 在**任务开始执行前**累积递减（start-based），链表末节点执行前强制 flush
 * ——递减必然先于该任务的完成信号落账；主线程经 rt_task_complete 的 release READY store
 * acquire 到完成态时，同步看到 pending 已归零。批量 fetch_sub（RT_PENDING_BATCH_SIZE）
 * 保留，cache line 争用降 16× 不变。
 *
 * wait_idle 不再单独依赖 pending_count：以「pending=0 且全部 worker busy=0 且 injector /
 * 全局队列无未取节点」判定空闲（见 rt_threadpool_wait_idle）。
 *
 * 排序：spawn 侧 add 用 release（add-before-push，任务可见前计数已落账）；worker 侧
 * fetch_sub 亦用 release——wait_idle 的 acquire 读到 sub 值即同步到 busy=1 之前的全部
 * worker 状态（闭合 ARM 弱序下「读到 pending=0 却漏看 busy=1」的间隙）。
 * wait_all 不依赖 pending_count（用 condvar+counter），不受影响。 */
#define RT_PENDING_BATCH_SIZE 16

static inline void worker_pending_sub_n(rt_worker_ctx* ctx, rt_threadpool* pool, int32_t n) {
    ctx->pending_batch += n;
    if (ctx->pending_batch >= RT_PENDING_BATCH_SIZE) {
        atomic_fetch_sub_explicit(&pool->pending_count, ctx->pending_batch,
                                  memory_order_release);
        ctx->pending_batch = 0;
    }
}

static inline void worker_pending_flush(rt_worker_ctx* ctx, rt_threadpool* pool) {
    if (ctx->pending_batch > 0) {
        atomic_fetch_sub_explicit(&pool->pending_count, ctx->pending_batch,
                                  memory_order_release);
        ctx->pending_batch = 0;
    }
}

/* 执行一条 work 链表（可能含 RT_TP_BATCH 个节点，或单节点），逐个执行 work.fn 后回收节点。
 * pending_count 按 start-based 递减：每个节点开始执行前累积 1，链表末节点执行前强制 flush
 * ——使全部递减在该链表最后一个 work 完成（work.fn → rt_task_complete → Task READY）之前
 * 落账，主线程 Wait() 返回后读到 PendingTaskCount=0。busy 标志覆盖整个执行窗口。 */
static void rt_run_work_list(rt_worker_ctx* ctx, rt_threadpool* pool, rt_work_node* node) {
    atomic_store_explicit(&ctx->busy, 1, memory_order_release);
    while (node) {
        rt_work_node* next =
            (rt_work_node*)(uintptr_t)atomic_load_explicit(&node->next, memory_order_relaxed);
        /* 计数在执行前递减（start-based）：完成后观察者即可读到准确计数 */
        worker_pending_sub_n(ctx, pool, 1);
        if (!next) worker_pending_flush(ctx, pool);   /* 末节点：递减先于完成落账 */
        /* RFC 009 §3：先执行后回收——work.data 可能指向 node 内部（如 rt_task_run）。 */
        if (node->work.fn) node->work.fn(node->work.data);
        work_node_recycle_local(pool, node);
        node = next;
    }
    atomic_store_explicit(&ctx->busy, 0, memory_order_release);
}

static int try_get_work(rt_worker_ctx* ctx, rt_work_t* out, rt_work_node** out_node) {
    rt_threadpool* pool = ctx->pool;
    if (out_node) *out_node = NULL;

    /* RFC 009 M1：0. LIFO slot 优先（cache 局部性最优，仅 owner 可读）。
     * **下钻优化（2026-08-07）**：先 relaxed **peek**，空则跳过 atomic_exchange RMW。
     * 自 round-robin 改全局 injector 后，仅 owner（spawn_local/continuation）写
     * lifo_slot，owner 正在执行（RUNNING）时 slot 空是常态（burst 无 continuation）。
     * 原实现每任务无条件下 atomic_exchange（x86 `lock xchg` ~20ns even 空）。peek
     * 命中才 exchange：burst 下省去该 RMW；peek 读到非空也仅多一次 load，无正确性
     * 影响（slot 仍是 owner 独占，skip 后下轮或 park double-check 会再读到）。
     * A5②：节点由调用方在 work.fn 执行后回收（RFC 009 §3 延迟 push）。 */
    if (atomic_load_explicit(&ctx->lifo_slot, memory_order_relaxed) != NULL) {
        void* slot_item = atomic_exchange_explicit(&ctx->lifo_slot, NULL, memory_order_acquire);
        if (slot_item) {
            rt_work_node* node = (rt_work_node*)slot_item;
            *out = node->work;
            if (out_node) *out_node = node;
            return 1;
        }
    }

    /* 1. 本地 deque LIFO pop（LIFO slot 未命中时）。
     * **下钻优化（2026-08-07）**：先 relaxed size peek，空则跳过 rt_ws_pop（其含
     * seq_cst fence + 多次 bottom 读写，~15ns even 空）。burst 下 deque 空是常态。 */
    if (rt_ws_deque_size(ctx->deque) > 0) {
        void* item = rt_ws_pop(ctx->deque);
        if (item) {
            rt_work_node* node = (rt_work_node*)item;
            *out = node->work;
            if (out_node) *out_node = node;
            return 1;
        }
    }

    /* 2'. 全局无锁 injector（RFC 009 §6：外部 spawn 入口；任意空闲 worker 可取）。
     *    单 pop（Vyukov dequeue）——不 backfill 到本地 deque，避免 push/pop 往返
     *    栅栏开销（批量 drain 实测净回归，见 rt_inject_pop 注释）。
     *
     *    **次序论证（2026-08-07 下钻）**：原次序为「本地 → steal → injector」，
     *    burst（单生产者连续 spawn）场景下 injector 是**唯一**工作源，而 steal 扫描
     *    12 个空 deque（每处 2 次 acquire load）在每次有用 injector pop 前都是纯浪费
     *    ——约 24 次原子 load/任务。将 injector 提到 steal **之前**：本地（lifo+deque）
     *    仍优先（保全 LIFO/cache 局部性语义），injector 命中即取，仅当 injector 也空
     *    才做 steal 扫描。burst 下消除 steal 浪费，混合负载下仅当本地与 injector 均空
     *    才 steal，负载均衡语义不变（tokio 本地→inject→steal 同序）。 */
    rt_work_node* inode = NULL;
    if (rt_inject_pop(pool, &inode)) {
        *out = inode->work;
        if (out_node) *out_node = inode;
        return 1;
    }

    /* 3'. 2 之后才 steal 其他 worker（随机起始 + 环形扫描）。
     *    仅当本地与 injector 均空才扫描——burst 下 injector 命中即返回，不付
     *    12 空 deque 的扫描浪费。 */
    int32_t n = pool->n_workers;
    /* 简单 XorShift 随机数（避免依赖 stdlib rand 的锁） */
    static _Atomic(uint32_t) g_seed = 1;
    uint32_t seed = atomic_fetch_add_explicit(&g_seed, 1, memory_order_relaxed);
    int32_t start = (int32_t)(seed % (uint32_t)n);
    for (int32_t i = 0; i < n; i++) {
        int32_t victim = (start + i) % n;
        if (victim == ctx->worker_id) continue;
        void* item = rt_ws_steal(pool->deques[victim]);
        if (item) {
            rt_work_node* node = (rt_work_node*)item;
            *out = node->work;
            if (out_node) *out_node = node;
            return 1;
        }
    }
    /* 4. mutex 全局队列（deque 溢出 / injector 罕见溢出转存；正常极少）。
     * 无锁空提示（global_size 原子读）：非空才取锁，避免空转 worker 每轮付
     * mutex 锁开销（~50-100ns，占 worker 每任务成本大头）。racy 读 0 仅导致
     * 额外一次 park（push 侧 wake_one_parked + park 双查仍闭合 lost-wakeup）。 */
    if (atomic_load_explicit(&pool->global_size, memory_order_acquire) == 0) {
        return 0;
    }
    rt_sync_lock(&pool->sync);
    int found = global_queue_pop_locked(pool, out, out_node);
    /* 若全局队列仍有剩余，唤醒其他 sleeping worker 协同消费。
     * 避免 100 work 入队时仅 1 worker 被 signal 唤醒的饥饿问题
     *（condvar signal 会合并，多次 spawn 仅唤醒 1 worker）。 */
    if (found && atomic_load_explicit(&pool->global_size, memory_order_relaxed) > 0) {
        rt_sync_signal(&pool->sync);
    }
    rt_sync_unlock(&pool->sync);
    return found;
}

#if defined(_WIN32)
static unsigned __stdcall worker_main(void* arg) {
#else
static void* worker_main(void* arg) {
#endif
    rt_worker_ctx* ctx = (rt_worker_ctx*)arg;
    rt_threadpool* pool = ctx->pool;
    rt_pool_set_worker_ctx(pool, ctx);

    /* RFC 009 M5：NUMA 绑定——worker 线程启动时绑定到指定 NUMA node。
     * 必须在线程自身内调用（pthread_setaffinity_np/SetThreadGroupAffinity
     * 操作当前线程）。numa_node < 0 表示未启用，跳过绑定。
     * 平台差异化：Linux 用 CPU_SET，Windows 用 GROUP_AFFINITY，
     * macOS/其他平台 rt_numa_bind_worker 为 no-op。 */
    if (ctx->numa_node >= 0) {
        rt_numa_bind_worker(ctx->worker_id, ctx->numa_node);
    }

    while (1) {
        rt_work_t work;
        rt_work_node* node = NULL;
        if (try_get_work(ctx, &work, &node)) {
            /* RFC 009 M2：记录当前 task 执行起始时间（供定时器轮抢占检测）。
             * exec_start_ms 在主循环外层记录，覆盖 task 执行期间。
             * 抢占检查见 codegen 生成的 await 边界（rt_preempt_check）。 */
            rt_preempt_record_start(&ctx->preempt, rt_tp_now_ms());
            if (ctx->worker_id < RT_DIAG_MAX_WORKERS) {
                atomic_store_explicit(&g_diag_busy_since[ctx->worker_id],
                                      rt_tp_now_ms(), memory_order_relaxed);
                atomic_store_explicit(&g_diag_busy_fn[ctx->worker_id],
                                      (uintptr_t)(void*)work.fn, memory_order_relaxed);
            }
            /* 执行 work（RFC 038 §3：先执行后回收——work.data 可能指向 node 内部，
             * 如 rt_task_run 的 work.data = node；若先 push 回池被其他线程复用，
             * trampoline 访问 node->action_fn 等字段会 UAF）。
             * RFC 038 下钻：node 可能是生产者批的链表头（next != NULL），
             * rt_run_work_list 整链 drain（单节点亦兼容：next==NULL 循环一次）。 */
            if (node) rt_run_work_list(ctx, pool, node);
            if (ctx->worker_id < RT_DIAG_MAX_WORKERS) {
                atomic_store_explicit(&g_diag_busy_since[ctx->worker_id],
                                      0, memory_order_relaxed);
            }
            continue;
        }

        /* 无立即 work → 刷新 pending_batch（start-based 下通常已由末节点 flush 落账，此处
         * 为防御性兜底）：worker 从「连续执行」转入可能 idle 的窗口即 flush，避免任何路径
         * 遗漏的批滞留（batching 收益保留）。 */
        worker_pending_flush(ctx, pool);

        /* 无 work → 有界自旋吸收 burst（RFC 009 M5 ws 派发预算，2026-08-04）。
         * 背景：worker 每完成一个 noop 即 re-park，spawner 每次 spawn 都付一次
         * 完整 condvar 唤醒交接（~400ns，含线程唤醒/锁交接）。burst 场景 worker
         * 在自旋窗口内保持非 park → spawner 走快路径 2（无唤醒）→ 延迟降至
         * ~百 ns 级（与 wait_idle 既有有界 pause 自旋同策）。
         * 自旋仅轻量轮询 lifo_slot/deque（原子 load ~3ns/轮），不取全局锁；
         * 超限再进 park。lost-wakeup 不变式保持：自旋窗口内 try_get_work 会取到
         * 已投递 work；窗口外 park 协议（mark_parked → double-check → wait）不变。 */
        int spin = 0;
        for (; spin < RT_TP_SPIN_LIMIT; spin++) {
#if defined(_M_X64) || defined(_M_IX86) || defined(__x86_64__) || defined(__i386__)
            _mm_pause();
#endif
            if (atomic_load_explicit(&ctx->lifo_slot, memory_order_acquire) != NULL) break;
            if (rt_ws_deque_size(ctx->deque) > 0) break;
        }
        if (spin < RT_TP_SPIN_LIMIT) continue;  /* 检测到活 → 回主循环 try_get_work */

        {
        /* 无 work → park（per-worker condvar，无 thundering herd）。
         *
         * 2026-08-03 实测：pool 级 broadcast 每次 spawn 唤醒全部 worker，
         * task_spawn_wait 4-worker 场景 3 个空醒 + 重 park ≈ 2200ns/op；
         * 改为每个 worker 在**自己的** park_sync 上等待，spawner 只 signal
         * 目标 worker。lost-wakeup 由「signal 前持 ps 锁」协议闭合：worker
         * 进入 wait 时原子释放 ps 锁，spawner 拿到 ps 锁时 worker 必在
         * wait set 中（或已找到 work 清除标记）。
         *
         * RFC 038（2026-08-07）三态化：mark_parked CAS RUNNING→PARKED，若
         * spawner 已提前 notify（NOTIFIED 态）则 CAS 失败、消费 notification
         * 回退 RUNNING——worker 不 wait，避免「spawner 已 signal 但 worker
         * 刚要 wait」的冗余唤醒。did_park 标记 mark_parked 是否真 park。 */
            rt_sync_t* ps = (rt_sync_t*)ctx->park_sync;
            if (!ps) ps = &pool->sync;  /* 防御：极端未分配时回退 pool sync */
            rt_sync_lock(ps);
            /* RFC 038 §4: park 前刷新 pending_batch，确保 wait_idle 准确性 */
            worker_pending_flush(ctx, pool);
            /* RFC 009 M1 + 2026-08-03 早期标记协议：**先**标记 is_parked，再 double-check。
             * spawn 快路径（外部 spawn / LIFO 直接投递）观察 is_parked=PARKED 即唤醒；
             * 本窗口内 worker 必然进入 wait（被唤醒）或找到 work（清除标记），
             * 消除「spawner 已投递 LIFO slot、worker 才刚过 double-check 便 park」的
             * lost wakeup（P0 task_spawn_wait 挂起同类根因）。 */
            rt_worker_mark_parked(ctx);
            /* RFC 009：检查 mark_parked 是否真 park（CAS RUNNING→PARKED 成功）。
             * 若 NOTIFIED 被消费（is_parked=RUNNING），spawner 已 notify 过——
             * 不 wait（避免 lost notification），但仍 double-check 捡漏。 */
            int did_park = (atomic_load_explicit(&ctx->is_parked, memory_order_acquire)
                            == RT_WORKER_PARKED);
            /* double-check 避免 lost wakeup（LIFO slot + deque + global 全空） */
            rt_work_t work2;
            rt_work_node* node2 = NULL;
            /* RFC 009 M1：先检查 LIFO slot（与 try_get_work 一致顺序） */
            void* slot_item = atomic_exchange_explicit(&ctx->lifo_slot, NULL, memory_order_acquire);
            if (slot_item) {
                rt_worker_mark_unparked(ctx);
                rt_sync_unlock(ps);
                node2 = (rt_work_node*)slot_item;
                /* RFC 009 §3：先执行后回收（链表批整链 drain） */
                rt_run_work_list(ctx, pool, node2);
                continue;
            }
            /* double-check 全局无锁 injector（RFC 038 §6；lost-wakeup 闭合同
             * try_get_work 顺序：worker 持 ps 锁期间 spawner 无法 signal，必见或必被唤醒）
             * 单 pop——不 backfill 到本地 deque（批量 drain 实测净回归）。 */
            {
                rt_work_node* inode = NULL;
                if (rt_inject_pop(pool, &inode)) {
                    rt_worker_mark_unparked(ctx);
                    rt_sync_unlock(ps);
                    /* RFC 009 §3：先执行后回收（链表批整链 drain） */
                    rt_run_work_list(ctx, pool, inode);
                    continue;
                }
            }
            /* double-check 全局队列（pool->sync 保护；锁序 ps → pool->sync，
             * spawner 唤醒仅持 ps、慢路径仅持 pool->sync → 无 ABBA） */
            rt_sync_lock(&pool->sync);
            int found = global_queue_pop_locked(pool, &work2, &node2);
            rt_sync_unlock(&pool->sync);
            if (found) {
                rt_worker_mark_unparked(ctx);
                rt_sync_unlock(ps);
                /* RFC 009 §3：先执行后回收（链表批整链 drain） */
                if (node2) rt_run_work_list(ctx, pool, node2);
                continue;
            }
            if (atomic_load_explicit(&pool->shutdown, memory_order_acquire)) {
                rt_worker_mark_unparked(ctx);
                rt_sync_unlock(ps);
                worker_pending_flush(ctx, pool);  /* RFC 009 §4: 退出前刷新 */
                break;
            }
            if (did_park) {
                atomic_fetch_add_explicit(&g_diag_park_waits, 1, memory_order_relaxed);
                /* 真正 park：wait condvar（spawner signal 会唤醒）。
                 * 兜底超时（channels case2 丢失唤醒实证）：三态 park 协议
                 * 存在 notify 消费窗口——全部 worker 已 wait 而 injector
                 * 遗留 work 无人拉取的现场曾稳定复现。50ms 心跳把任何
                 * lost-wakeup 的永挂收敛为一次空醒（worker 醒来
                 * double-check 拉取遗留 work），性能代价可忽略
                 * （空闲 worker 每 50ms 一次自醒）。 */
                rt_sync_wait_timeout(ps, 50);
                atomic_fetch_add_explicit(&g_diag_hb_wakes, 1, memory_order_relaxed);
                rt_diag_maybe_dump();
                rt_worker_mark_unparked(ctx);
            }
            /* else: NOTIFIED 被消费（mark_parked 已回退 RUNNING）——不 wait，
             * 回主循环重新 try_get_work（spawner 投递的任务应已可见或即将可见，
             * worker 自旋窗口会捡到）。 */
            rt_sync_unlock(ps);
        }
    }

#if defined(_WIN32)
    return 0;
#else
    return NULL;
#endif
}

/* ---- ABI 实现 ---- */

/* 硬件并发数：默认池 worker 数（A5③ · RFC 016 §1.2，修复 rt_abi.h L1417 声称
 * 「n_workers<=0 → hardware_concurrency」与实际硬编码 4 的矛盾）。
 * Windows 用 GetActiveProcessorCount(ALL_PROCESSOR_GROUPS)（跨处理器组全逻辑核，
 * 优于 GetSystemInfo 仅组 0）；POSIX 用 sysconf(_SC_NPROCESSORS_ONLN)。返回至少 1。 */
static int32_t rt_tp_hardware_concurrency(void) {
#ifdef _WIN32
    DWORD n = GetActiveProcessorCount(ALL_PROCESSOR_GROUPS);
    return (n > 0) ? (int32_t)n : 1;
#else
    long n = sysconf(_SC_NPROCESSORS_ONLN);
    return (n > 0) ? (int32_t)n : 1;
#endif
}

rt_threadpool* rt_threadpool_create(int32_t n_workers, int32_t numa_aware) {
    /* RFC 009 M5：numa_aware 启用 NUMA 感知调度。
     * 平台差异化处理：
     *   - Linux/Windows：查询 NUMA 拓扑，将 worker 绑定到对应 node 的 CPU 集合
     *   - macOS/其他：rt_numa_bind_worker 为 no-op，numa_aware 标志被静默忽略
     * 启用后每个 worker 线程启动时调用 rt_numa_bind_worker 绑定 CPU 亲和性，
     * 实现first-touch 内存本地性 + 减少跨 socket 通信。 */
    int32_t numa_nodes = 0;
    if (numa_aware) {
        numa_nodes = rt_numa_node_count();
        /* numa_nodes <= 1 表示单 NUMA node 或平台不支持——无需绑定 */
    }

    if (n_workers <= 0) {
        /* 默认：硬件并发数（A5③ · RFC 016 §1.2；原硬编码 4 与 rt_abi.h L1417
         * 「n_workers<=0 → hardware_concurrency」矛盾，已接入真实硬件并发）。 */
        n_workers = rt_tp_hardware_concurrency();
    }
    if (n_workers > 64) n_workers = 64;  /* 上限保护 */
    if (n_workers < 1) n_workers = 1;    /* 下限保护（探测异常返回时） */

    rt_threadpool* pool = RT_OPAQUE_NEW(rt_threadpool);
    if (!pool) return NULL;
    /* RT_OPAQUE_NEW 不清零——workers/ctxs/deques 的 NULL 预检依赖零初值 */
    memset(pool, 0, sizeof(*pool));

    pool->n_workers = n_workers;
    pool->workers = (rt_worker_thread*)calloc((size_t)n_workers, sizeof(rt_worker_thread));
    pool->worker_ctxs = (rt_worker_ctx*)calloc((size_t)n_workers, sizeof(rt_worker_ctx));
    pool->deques = (rt_ws_deque**)calloc((size_t)n_workers, sizeof(rt_ws_deque*));
    if (!pool->workers || !pool->worker_ctxs || !pool->deques) {
        free(pool->workers); free(pool->worker_ctxs); free(pool->deques); rt_obj_free(pool);
        return NULL;
    }

    rt_sync_init(&pool->sync);
    atomic_init(&pool->pending_count, 0);
    atomic_init(&pool->shutdown, 0);
    atomic_init(&pool->work_pool.free_head, 0);  /* A5② work-node 池 */
    if (rt_inject_init(pool) != 0) {
        free(pool->workers); free(pool->worker_ctxs); free(pool->deques); rt_obj_free(pool);
        return NULL;
    }
    rt_pool_init_tls(pool);

    /* 注册 overflow handler（函数指针全局；池指针 per-deque ctx） */
    rt_ws_deque_set_overflow_handler(ws_overflow_handler);

    /* 为每个 worker 创建 deque + 上下文 */
    for (int32_t i = 0; i < n_workers; i++) {
        pool->deques[i] = rt_ws_deque_create(i, 16);  /* cap = 2^16 = 65536 */
        rt_ws_deque_set_overflow_ctx(pool->deques[i], pool);
        pool->worker_ctxs[i].worker_id = i;
        pool->worker_ctxs[i].deque = pool->deques[i];
        pool->worker_ctxs[i].pool = pool;
        /* RFC 009 M1：初始化 LIFO slot + park 状态 */
        atomic_init(&pool->worker_ctxs[i].lifo_slot, (void*)NULL);
        atomic_init(&pool->worker_ctxs[i].is_parked, 0);
        atomic_init(&pool->worker_ctxs[i].busy, 0);
        /* RFC 009 M5（2026-08-03）：per-worker park 同步原语（无 thundering herd）。
         * 不 free（H1 漏至进程退出，与池结构一致）。 */
        rt_sync_t* ps = (rt_sync_t*)malloc(sizeof(rt_sync_t));
        if (!ps) {
            pool->n_workers = i;
            break;
        }
        rt_sync_init(ps);
        pool->worker_ctxs[i].park_sync = ps;
        /* RFC 009 M2：初始化异步抢占状态 */
        atomic_init(&pool->worker_ctxs[i].preempt.preempt_requested, 0);
        atomic_init(&pool->worker_ctxs[i].preempt.exec_start_ms, 0);
        /* RFC 009 M5：NUMA 绑定——将 worker i 映射到 node (i % numa_nodes)。
         * 轮询分配保证 worker 在各 NUMA node 间均衡分布。
         * numa_nodes <= 1 时设 -1 表示未启用（worker_main 跳过绑定）。 */
        if (numa_aware && numa_nodes > 1) {
            pool->worker_ctxs[i].numa_node = i % numa_nodes;
        } else {
            pool->worker_ctxs[i].numa_node = -1;
        }
    }

    /* 启动 worker 线程 */
    for (int32_t i = 0; i < n_workers; i++) {
        pool->workers[i].id = i;
        if (rt_tp_thread_create(&pool->workers[i].handle, &pool->worker_ctxs[i]) != 0) {
            /* 启动失败：缩小 worker 数 */
            pool->n_workers = i;
            break;
        }
    }

    return pool;
}

void rt_threadpool_destroy(rt_threadpool* pool) {
    if (!pool) return;

    /* Safe destroy（L2 Stable）：先排空 pending，再 join 仍存活的 worker。
     * 可在 Shutdown() 之后调用（跳过已 NULL 的 handle，避免 Windows WaitForSingleObject(NULL)）。
     * 禁止重复 Destroy（二次 join 已 NULL 句柄为 no-op）。 */
    rt_threadpool_wait_idle(pool);

    if (!atomic_load_explicit(&pool->shutdown, memory_order_acquire)) {
        atomic_store_explicit(&pool->shutdown, 1, memory_order_release);
        /* 唤醒全部 worker：逐个 signal per-worker park_sync（worker 已不再在
         * pool->sync condvar 上等待）。 */
        for (int32_t i = 0; i < pool->n_workers; i++) {
            rt_sync_t* ps = (rt_sync_t*)pool->worker_ctxs[i].park_sync;
            if (ps) {
                rt_sync_lock(ps);
                rt_sync_broadcast(ps);
                rt_sync_unlock(ps);
            }
        }
        rt_sync_lock(&pool->sync);
        rt_sync_broadcast(&pool->sync);
        rt_sync_unlock(&pool->sync);
    }

    for (int32_t i = 0; i < pool->n_workers; i++) {
#ifdef _WIN32
        if (pool->workers[i].handle) {
            rt_tp_thread_join(pool->workers[i].handle);
            pool->workers[i].handle = NULL;
        }
#else
        if (pool->workers[i].handle) {
            rt_tp_thread_join(pool->workers[i].handle);
            pool->workers[i].handle = (pthread_t)0;
        }
#endif
    }

    /* H1: Join 后**不** free LIFO/deque/池结构。UnitTest 多池 Destroy 与
     * Task.Run 默认池 / WriteResults CRT 分配交织时，中途 free 放大为
     * flaky 0xC0000005（末条 Wiki_Snapshot_Restore 截断）。漏至进程退出。
     * 摘 overflow_ctx，杜绝迟到 push 写入本池。 */
    for (int32_t i = 0; i < pool->n_workers; i++) {
        (void)atomic_exchange_explicit(&pool->worker_ctxs[i].lifo_slot, NULL,
                                       memory_order_acquire);
        rt_ws_deque_set_overflow_ctx(pool->deques[i], NULL);
    }
}

void rt_threadpool_spawn(rt_threadpool* pool, rt_work_t work) {
    /* RFC 009 §6（2026-08-07）：外部 spawn → 全局无锁 MPMC injector。
     *
     * 演进（对齐 tokio Injector / Rayon inject / Go global runq 本质）：
     *   - RFC 009 M1：per-worker condvar（消除 thundering herd）
     *   - RFC 005：parked_count 门控跳过 O(n_workers) 扫描
     *   - RFC 009：is_parked 三态化（消除 redundant signal）
     *   - RFC 009 §6：round-robin 投递 worker 私有 lifo_slot → 全局无锁 injector。
     *     原 round-robin 每 spawn 一次 atomic_exchange 把 worker slot 的 cache line
     *     从 worker CPU 抢到 spawner CPU（RFO 乒乓 ~80-160ns/op），且随机投递无法
     *     负载均衡。injector 让 spawner 只写单条热 cache line，worker 本地空时拉取。
     *
     * 唤醒：push 后仅当确有 parked worker（parked_count>0）才 wake_one_parked；
     * burst 忙碌期（parked_count==0）跳过一次 load 即返回。park 双查闭合 lost-wakeup。 */
    rt_work_node* node = work_node_alloc(pool);
    if (node) {
        node->work = work;
        /* add-before-push：任务可见前计数已落账（start-based，见 rt_run_work_list 说明） */
        atomic_fetch_add_explicit(&pool->pending_count, 1, memory_order_release);
        rt_inject_push(pool, node);
        if (atomic_load_explicit(&pool->parked_count, memory_order_acquire) > 0) {
            wake_one_parked(pool);
        }
        return;
    }
    /* OOM：无法分配节点 → 丢弃 work（与旧慢路径同策；injector 有界满时自旋不触发） */
}

/* rt_work_node_alloc / rt_work_node_recycle：暴露 work_pool 给 rt_task_run.c，
 * 消除 rt_rw_pool（RFC 009 §3：单 work_pool 分配，从 3 次 CAS 降至 2 次）。
 * node 统一大小（含 task/action_fn/action_data/ct 扩展字段），普通 work 不使用扩展字段。 */
rt_work_node* rt_work_node_alloc(rt_threadpool* pool) {
    return work_node_alloc(pool);
}

void rt_work_node_recycle(rt_threadpool* pool, rt_work_node* node) {
    if (pool && node) work_pool_push(pool, node);
}

/* rt_threadpool_spawn_node：预分配 node 的 spawn 快路径（rt_task_run 路径）。
 * node 须来自 rt_work_node_alloc；worker 执行完后自动回收（work_pool_push）。 */
void rt_threadpool_spawn_node(rt_threadpool* pool, rt_work_node* node) {
    if (!pool || !node) return;

    /* RFC 009 §6（2026-08-07）：外部 spawn → 全局无锁 MPMC injector。
     *
     * 原 round-robin 直接 atomic_exchange 到 worker 私有 lifo_slot：spawner 每任务
     * 把 slot cache line 从 worker CPU 抢到 spawner CPU 再抢回（RFO 乒乓），且
     * 随机投递可能压到繁忙 worker。对齐 tokio Injector / Rayon inject / Go global
     * runq 本质：push 只写 injector 单条热 cache line（无乒乓），任意空闲 worker
     * 拉取（负载均衡）。实测 round-robin(401ns) > mutex 队列(1183ns)，lock-free
     * injector 是唯一能超越 round-robin 的（.NET ConcurrentQueue 同构）。 */
    /* add-before-push：任务可见前计数已落账（start-based，见 rt_run_work_list 说明） */
    atomic_fetch_add_explicit(&pool->pending_count, 1, memory_order_release);
    rt_inject_push(pool, node);

    /* 唤醒任一 parked worker（仅当确有 parked；burst 忙碌期 parked_count==0 跳过）。
     * wake_one_parked 内部做 PARKED→NOTIFIED CAS，成功才 signal（消除 redundant
     * signal）。park 双查（含 injector）闭合 lost-wakeup。 */
    if (atomic_load_explicit(&pool->parked_count, memory_order_acquire) > 0) {
        wake_one_parked(pool);
    }
}

/* rt_threadpool_spawn_node_batched（RFC 009 下钻 · 2026-08-07）：预分配 node 的
 * 批式 spawn（rt_task_run 路径）。节点先入当前线程 TLS 批，批满（RT_TP_BATCH）
 * 才一次注入器 push 发布——把 injector 槽位 cold-write 从 N 次降至 1 次、dequeue
 * CAS 争用降低 ~RT_TP_BATCH 倍（对齐 .NET ConcurrentQueue 段 / Go per-P runq）。
 *
 * 正确性契约：凡本线程随后执行阻塞等待（rt_task_wait_all/wait_timeout/wait_ct/
 * wait_any、rt_threadpool_wait_idle、worker park），必须先 rt_threadpool_flush_local()
 * 冲刷尾批，保证「已 spawn 必已发布」。同线程 spawn+wait 恒正确（本机真实用法）。
 *
 * 2026-08-21 fire-and-forget 滞留修复：非 worker 线程（如 WebApplication accept
 * 循环——持续 spawn 且永不等待/退出的线程）的尾批只能靠批满（RT_TP_BATCH）或
 * 线程退出析构发布——accept 下首批连接的任务滞留到第 32 个连接才执行（首个请求
 * 挂起）。批优化收益在非 worker 线程本就不存在（无 deque LIFO 局部性），改为
 * **直接发布**（pending_count add-before-push + injector push + 唤醒），保证
 * fire-and-forget 任务及时执行；worker 线程内派生保持批式（热路径性能）。 */
void rt_threadpool_spawn_node_batched(rt_threadpool* pool, rt_work_node* node) {
    if (!pool || !node) return;
    if (!rt_pool_get_worker_ctx(pool)) {
        atomic_store_explicit(&node->next, NULL, memory_order_relaxed);
        atomic_fetch_add_explicit(&pool->pending_count, 1, memory_order_release);
        rt_inject_push(pool, node);
        if (atomic_load_explicit(&pool->parked_count, memory_order_acquire) > 0) {
            wake_one_parked(pool);
        }
        return;
    }
    rt_tp_batch_append(pool, node);
}

void rt_threadpool_spawn_local(rt_threadpool* pool, rt_work_t work) {
    /* worker 本地 spawn → 优先 LIFO slot，已占用则溢出到 deque */
    rt_worker_ctx* ctx = rt_pool_get_worker_ctx(pool);
    if (!ctx) {
        /* 非 worker 线程调用 → fallback 到全局队列 */
        rt_threadpool_spawn(pool, work);
        return;
    }

    /* RFC 009 M1：优先 push LIFO slot（cache 局部性 + continuation 派发延迟 <15ns）。
     * rt_worker_push_lifo 内部处理：slot 已占用则 atomic_exchange 溢出到 deque。
     * add-before-push：任务可见前计数已落账（start-based，见 rt_run_work_list 说明）。 */
    atomic_fetch_add_explicit(&pool->pending_count, 1, memory_order_release);
    rt_worker_push_lifo(ctx, work);

    /* RFC 009 M1：needs_wakeup 快路径——LIFO slot 空时才唤醒（避免无效唤醒）。
     * 原 M5.1 行为：无条件 signal → 多次 spawn 仅唤醒 1 worker（condvar signal 合并），
     * 但 worker 醒来后发现 LIFO slot 非空（已有活干）即继续执行，不进入 park。
     * M1 改为：slot 空时 signal（worker 可能 parked 需唤醒）；slot 非空跳过（已有活干）。
     * 2026-08-03：唤醒目标改为任一 parked worker 的 per-worker cv（原 pool sync signal
     * 在 per-worker park 协议下不再唤醒任何 worker）。 */
    if (rt_worker_needs_wakeup(ctx)) {
        wake_one_parked(pool);
    }
}

int32_t rt_threadpool_worker_id(void) {
    /* 真 TLS worker_ctx；非 worker 返回 -1。 */
    rt_worker_ctx* ctx = g_tls_worker_ctx;
    return ctx ? ctx->worker_id : -1;
}

int32_t rt_threadpool_pending_count(rt_threadpool* pool) {
    return atomic_load_explicit(&pool->pending_count, memory_order_acquire);
}

int32_t rt_threadpool_worker_count(rt_threadpool* pool) {
    /* 返回池中实际 worker 数。Parallel.For 用此值估算分区数；
     * n_workers 在 create 时确定（hardware_concurrency 或用户指定）。 */
    return pool ? pool->n_workers : 0;
}

/* wait_idle 的「仍有未完成 work」判定。start-based 下 pending_count 只覆盖「排队未开始」，
 * 故须叠加执行窗口（worker busy）与排队未取窗口（injector / 全局队列）才能判定空闲：
 *   - pending>0：有任务已排队未开始执行；
 *   - 任一 worker busy=1：正在执行 work（此刻其 pending 递减可能尚未落账）；
 *   - injector / global_size 非空：已发布未取（worker 尚未 pop）。
 * 三者合取为 0 才允许返回——等价的「调度器空闲即全部 work 完成」。 */
static int rt_threadpool_has_inflight(rt_threadpool* pool) {
    if (atomic_load_explicit(&pool->pending_count, memory_order_acquire) > 0) return 1;
    for (int32_t i = 0; i < pool->n_workers; i++) {
        if (atomic_load_explicit(&pool->worker_ctxs[i].busy, memory_order_acquire)) return 1;
    }
    rt_injector* q = &pool->injector;
    if (atomic_load_explicit(&q->dequeue_pos, memory_order_relaxed) <
        atomic_load_explicit(&q->enqueue_pos, memory_order_relaxed)) return 1;
    if (atomic_load_explicit(&pool->global_size, memory_order_relaxed) > 0) return 1;
    return 0;
}

void rt_threadpool_wait_idle(rt_threadpool* pool) {
    /* RFC 009 下钻：先冲刷当前线程生产者批——同线程 spawn 后 wait 时，尾批
     * （< RT_TP_BATCH）可能滞留于 TLS 批，不冲刷则 pending_count 未计入、wait 空转。 */
    rt_threadpool_flush_local();
    /* 等待所有 pending work 完成（用于测试）。
     * 生产代码应使用 Task await；此函数阻塞调用线程。
     *
     * 2026-08-03 快排（RFC 009 M5 ws 派发预算）：burst 工作负载下 Sleep(0) 的
     * ~µs 级 yield 粒度是 wait_idle 排空期主成本（work_stealing_latency 测时含
     * shutdown→wait_idle）。先做有界 pause 自旋（~ns 级），仍非空再 Sleep(0)。 */
    while (rt_threadpool_has_inflight(pool)) {
#ifdef _WIN32
        int spins = 0;
        while (rt_threadpool_has_inflight(pool) && spins < 256) {
#if defined(_M_X64) || defined(_M_IX86) || defined(__x86_64__) || defined(__i386__)
            _mm_pause();
#endif
            spins++;
        }
        if (rt_threadpool_has_inflight(pool)) {
            Sleep(0);  /* yield */
        }
#else
        sched_yield();
#endif
    }
}

void rt_threadpool_shutdown(rt_threadpool* pool) {
    /* 关闭：wait_idle → 停 worker（join）→ 解除全局溢出指针。
     * 不 free 池结构；完整堆回收见 rt_threadpool_destroy（Arc Destroy）。
     * L2 Stable：Shutdown = 停池；Destroy = Shutdown 语义 + free。 */
    if (!pool) return;
    rt_threadpool_wait_idle(pool);

    atomic_store_explicit(&pool->shutdown, 1, memory_order_release);
    /* 唤醒全部 worker：逐个 signal per-worker park_sync */
    for (int32_t i = 0; i < pool->n_workers; i++) {
        rt_sync_t* ps = (rt_sync_t*)pool->worker_ctxs[i].park_sync;
        if (ps) {
            rt_sync_lock(ps);
            rt_sync_broadcast(ps);
            rt_sync_unlock(ps);
        }
    }
    rt_sync_lock(&pool->sync);
    rt_sync_broadcast(&pool->sync);
    rt_sync_unlock(&pool->sync);

    for (int32_t i = 0; i < pool->n_workers; i++) {
#ifdef _WIN32
        if (pool->workers[i].handle) {
            rt_tp_thread_join(pool->workers[i].handle);
            pool->workers[i].handle = NULL;
        }
#else
        if (pool->workers[i].handle) {
            rt_tp_thread_join(pool->workers[i].handle);
            pool->workers[i].handle = (pthread_t)0;
        }
#endif
    }

}

/* ---- RFC 009 M1: LIFO slot ABI 实现 ----
 *
 * 设计要点：
 *   - lifo_slot 存 rt_work_node* 指针（A5② 起；deque/slot/全局队列统一 work 节点）
 *   - push 用 atomic_exchange：旧值非 NULL 则溢出（owner→本地 deque；非 owner→全局队列）
 *   - pop 用 atomic_exchange NULL：仅 owner 调用，无 CAS 争用
 *   - needs_wakeup 用 atomic_load：LIFO slot 空 + is_parked 时才 signal
 *   - mark_parked/mark_unparked 用 atomic_store：worker park 前后调用
 *
 * 性能目标（RFC 009 §0.2）：
 *   - rt_worker_push_lifo: ~5ns（atomic_exchange 无锁）
 *   - rt_worker_needs_wakeup: ~2ns（atomic_load relaxed）
 *   - continuation 派发延迟（push + pop + needs_wakeup）: <15ns
 *
 * 不可偷规则（RFC 009 §6.1）：
 *   - stealer 仅偷本地 deque 的 FIFO 端（rt_ws_steal）
 *   - LIFO slot 不参与 steal，仅 owner 可消费
 *   - LIFO slot 溢出到本地 deque 仅当 owner 自己 push 时 slot 已占用；
 *     非 owner push（spawn 快路径）的溢出 work 一律走全局队列（P0 修复） */

rt_worker_ctx* rt_threadpool_current_worker_ctx(void) {
    /* 真 TLS；与全局 current-pool 解耦。 */
    return g_tls_worker_ctx;
}

void rt_worker_push_lifo(rt_worker_ctx* w, rt_work_t work) {
    /* 包装为 work 节点（A5②：优先回池复用，池空才 malloc） */
    rt_work_node* node = work_node_alloc(w->pool);
    if (!node) return;  /* OOM：丢弃 work（与原 spawn_local 一致行为） */
    node->work = work;

    /* atomic_exchange：新值入 slot，旧值返回。
     * - 旧值 NULL：slot 原本为空，新 work 入 slot（cache 局部性最优）
     * - 旧值非 NULL：slot 已占用，旧 work 溢出（保持 FIFO 顺序） */
    void* old = atomic_exchange_explicit(&w->lifo_slot, node, memory_order_release);
    if (old) {
        if (w == rt_threadpool_current_worker_ctx()) {
            /* owner push（spawn_local 快路径）→ 本地 deque。
             * Chase-Lev 不变式「仅 owner push」：owner 无锁读写 bottom 安全。 */
            rt_ws_push(w->deque, old);
        } else {
            /* 非 owner push（rt_threadpool_spawn 快路径投递给 parked worker）：
             * 溢出 work 路由到全局队列，**禁止**写入他人 deque——
             * 否则与 owner rt_ws_pop / stealer rt_ws_steal 并发竞争 bottom/top，
             * 违反 Chase-Lev 不变式，任务可滞留永不执行（P0 task_spawn_wait 挂起）。 */
            ws_overflow_handler(w->pool, old);
        }
    }
}

int32_t rt_worker_needs_wakeup(rt_worker_ctx* w) {
    /* LIFO slot 空时才 signal（避免无效唤醒）。
     * - slot 空 + worker 可能 parked → signal 唤醒
     * - slot 非空：worker 已有活干，跳过 signal（避免 spurious wake）
     *
     * 命名说明（RFC 009 §2.2.2 评审修订）：
     *   原 should_notify 语义易误读为「应该通知有新任务」，
     *   实际语义是「worker 是否需要被唤醒」。改名 needs_wakeup 更清晰。 */
    void* slot = atomic_load_explicit(&w->lifo_slot, memory_order_acquire);
    return (slot == NULL) ? 1 : 0;
}

void rt_worker_mark_parked(rt_worker_ctx* w) {
    /* worker 进入 park 前调用。RFC 009 三态化：CAS RUNNING→PARKED。
     *
     * 调用方（worker_main）已持有 park_sync 锁。CAS 语义：
     *   - 期望 RUNNING(0)→PARKED(1)：成功 → 真正 park，parked_count++。
     *   - 期望失败（旧值 NOTIFIED(2)）：spawner 已提前 notify（worker 在自旋→park
     *     窗口内被投递任务）→ 消费 notification，回退 RUNNING，parked_count--（因
     *     NOTIFIED 仍计入 parked_count，回退 RUNNING 须递减）。worker_main 据返回后
     *     is_parked != PARKED 判断「不需 wait」。
     *   - 旧值 PARKED(1)：不应发生（worker 单线程，不会重复 mark），防御性不处理。
     *
     * 消除 redundant signal 的关键：spawner CAS PARKED→NOTIFIED 后，worker 的
     * mark_parked CAS 必失败（NOTIFIED），从而不进入 wait——避免「spawner signal 一个
     * 已被通知的 worker」的冗余唤醒。 */
    int32_t expected = RT_WORKER_RUNNING;
    if (atomic_compare_exchange_strong_explicit(&w->is_parked, &expected,
            RT_WORKER_PARKED, memory_order_acq_rel, memory_order_relaxed)) {
        /* CAS 成功（RUNNING→PARKED）：parked_count++（统计非 RUNNING worker）。 */
        atomic_fetch_add_explicit(&((rt_threadpool*)w->pool)->parked_count, 1,
                                  memory_order_release);
    } else {
        /* CAS 失败：旧值必为 NOTIFIED(2)（worker 单线程不会重复 PARKED）。
         * 消费 notification：回退 RUNNING + parked_count--（NOTIFIED 之前已计入）。 */
        atomic_store_explicit(&w->is_parked, RT_WORKER_RUNNING, memory_order_release);
        rt_threadpool* pool = (rt_threadpool*)w->pool;
        int32_t old = atomic_load_explicit(&pool->parked_count, memory_order_relaxed);
        while (old > 0) {
            if (atomic_compare_exchange_weak_explicit(&pool->parked_count, &old, old - 1,
                    memory_order_release, memory_order_relaxed)) {
                break;
            }
        }
    }
}

void rt_worker_mark_unparked(rt_worker_ctx* w) {
    /* worker 唤醒后调用。RFC 009 三态化：从 PARKED 或 NOTIFIED 回退 RUNNING。
     * 调用方（worker_main）在 condvar wait 返回后（或 double-check 命中后）调用，
     * 已持 park_sync 锁。store RUNNING + parked_count--（守卫递减）。
     * NOTIFIED→RUNNING 也走此路径（worker 被 signal 唤醒后 is_parked 仍 NOTIFIED，
     * mark_unparked 统一回退 RUNNING）。 */
    atomic_store_explicit(&w->is_parked, RT_WORKER_RUNNING, memory_order_release);
    rt_threadpool* pool = (rt_threadpool*)w->pool;
    int32_t old = atomic_load_explicit(&pool->parked_count, memory_order_relaxed);
    while (old > 0) {
        if (atomic_compare_exchange_weak_explicit(&pool->parked_count, &old, old - 1,
                memory_order_release, memory_order_relaxed)) {
            break;
        }
    }
}

/* ---- Task.Run ABI (RFC 009 M5.7) —— 功能已迁移至 rt_task_run.c ---- */
/* rt_task_run / rt_task_run_on_pool 统一由 rt_task_run.c 提供。
 * rt_threadpool.c 仅保留 ThreadPool 核心职责（worker 管理、spawn、wait_idle 等）。
 * Task.Run trampoline 和默认线程池管理也由 rt_task_run.c 负责。 */
