// Task slab allocator (RFC 009 M5.2).
//
// Per-worker lock-free free-list for RtTask allocation.
// Goal: eliminate malloc on Task creation/release hot path.
//
// ## 设计
//
// 每个 worker 线程通过 TLS 持有一个 rt_task_slab：
//   - free_list: 单链表头插头取，owner 线程无锁访问
//   - high_water: 高水位标记（监控用）
//   - cache padding: 64B 对齐避免 false sharing
//
// ## 生命周期
//
// 1. Worker 线程启动 → rt_task_slab_thread_init() 初始化 TLS slab
// 2. Task 创建 → rt_task_alloc() 优先从 TLS slab free_list 弹出
//    - 空则 malloc 一个新 RtTask（首次或高并发场景）
// 3. Task 释放 → rt_task_release() 推回 TLS slab free_list
// 4. Worker 线程退出 → rt_task_slab_thread_destroy() 释放所有待用 Task
//
// ## 性能模型（RFC 009 §17.5）
//
// - slab alloc: ~5ns（free_list pop + memset 清零关键字段）
// - slab free:  ~5ns（free_list push）
// - vs calloc:  ~80ns（kernel mmap/brk + zero page fault）
// - 目标：10⁶ Task cycle < 30ms

#include "rt_abi.h"
#include <stddef.h>  /* offsetof */
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#ifdef _WIN32
#include <windows.h>
#endif

/* ---- 丢失唤醒取证：活任务注册表（临时：随诊断计数器整体回收） ---- */
static RtTask* g_live_head = NULL;
static _Atomic(uint32_t) g_live_lock;
static _Atomic(uint64_t) g_live_count;

static void rt_live_register(RtTask* t) {
    uint32_t expected = 0;
    while (!atomic_compare_exchange_strong_explicit(&g_live_lock, &expected, 1u,
            memory_order_acquire, memory_order_relaxed)) {
        expected = 0;
    }
    if (t->diag_linked) {
        /* 已在链上：register 幂等保护（防双重登记制造环）。 */
        atomic_store_explicit(&g_live_lock, 0u, memory_order_release);
        return;
    }
    t->diag_prev = NULL;
    t->diag_next = g_live_head;
    if (g_live_head) g_live_head->diag_prev = t;
    g_live_head = t;
    t->diag_linked = 1;
    atomic_fetch_add_explicit(&g_live_count, 1, memory_order_relaxed);
    atomic_store_explicit(&g_live_lock, 0u, memory_order_release);
}

/* DOUBLE-UNREG 调用栈捕获（当前线程，CaptureStackBackTrace + 模块 RVA，
 * 离线 llvm-symbolizer/导出表对照符号化）。限次防刷屏。 */
static void rt_diag_unreg_btrace(void) {
#ifdef _WIN32
    static _Atomic(int32_t) unreg_stk_count;
    if (atomic_fetch_add_explicit(&unreg_stk_count, 1,
                                  memory_order_relaxed) >= 8) {
        return;
    }
    void* frames[16];
    USHORT n = CaptureStackBackTrace(0, 16, frames, NULL);
    HMODULE mod = NULL;
    char mod_path[MAX_PATH];
    for (USHORT i = 0; i < n; i++) {
        mod = NULL;
        if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS |
                                GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                                (LPCWSTR)frames[i], &mod) || !mod) {
            continue;
        }
        if (GetModuleFileNameA(mod, mod_path, MAX_PATH) <= 0) {
            continue;
        }
        const char* base = mod_path;
        for (const char* p = mod_path; *p; p++) {
            if (*p == '\\' || *p == '/') base = p + 1;
        }
        fprintf(stderr, "[unreg-stk] #%u %s+0x%llx\n", (unsigned)i, base,
                (unsigned long long)((char*)frames[i] - (char*)mod));
    }
#endif
}

static void rt_live_unregister(RtTask* t) {
    if (!t->diag_linked) {
        /* 链外任务不得摘链：否则 p=NULL 分支会把 g_live_head 抹成 NULL，
         * 链上其余任务全部失联（CHAIN-BROKEN：lc>0 而 live_walk=0 的根因）。
         * 触发即打印——调用方存在对同一任务的二次 free/unregister。 */
        fprintf(stderr, "[DOUBLE-UNREG] t=%p status=%d from_slab=%d "
                        "—— 对链外任务二次 unregister（double-free 路径）\n",
                (void*)t, atomic_load_explicit((_Atomic int32_t*)&t->status,
                                               memory_order_relaxed),
                t->from_slab);
        rt_diag_unreg_btrace();
        return;
    }
    uint32_t expected = 0;
    while (!atomic_compare_exchange_strong_explicit(&g_live_lock, &expected, 1u,
            memory_order_acquire, memory_order_relaxed)) {
        expected = 0;
    }
    RtTask* p = t->diag_prev;
    RtTask* n = t->diag_next;
    if (p) p->diag_next = n; else g_live_head = n;
    if (n) n->diag_prev = p;
    t->diag_prev = t->diag_next = NULL;
    t->diag_linked = 0;
    atomic_fetch_sub_explicit(&g_live_count, 1, memory_order_relaxed);
    atomic_store_explicit(&g_live_lock, 0u, memory_order_release);
}

/* 挂死普查：按 (status, await_waiting, waker, resume) 形态统计活任务分布。
 * out[0]=live 总数；out[1]=PENDING+bit+waker；out[2]=PENDING+bit+无waker（零唤醒源）；
 * out[3]=PENDING+无bit+waker；out[4]=PENDING+无bit+无waker+resume!=NULL；
 * out[5]=PENDING+resume==NULL（聚合器/Task.Run 壳）；out[6]=终态未回收；
 * out[7]=POLLING 持有中。诊断读取容忍与活写的良性数据竞争。 */
void rt_diag_task_census(unsigned long long* out) {
    for (int i = 0; i < 8; i++) out[i] = 0;
    uint32_t expected = 0;
    while (!atomic_compare_exchange_strong_explicit(&g_live_lock, &expected, 1u,
            memory_order_acquire, memory_order_relaxed)) {
        expected = 0;
    }
    RtTask* t = g_live_head;
    while (t) {
        out[0]++;
        int32_t st = atomic_load_explicit((_Atomic(int32_t)*)&t->status,
                                          memory_order_relaxed);
        uint32_t bit = atomic_load_explicit((_Atomic(uint32_t)*)&t->await_waiting,
                                            memory_order_relaxed);
        uint32_t pf = atomic_load_explicit((_Atomic(uint32_t)*)&t->poll_flags,
                                           memory_order_relaxed);
        int has_waker = (t->waker != NULL);
        if (st == RT_TASK_PENDING) {
            /* PENDING 全清单（限量）：产者任务形态核验 + canceled 标志追踪
             * （round=5 线索：canceled=1 分支不 store READY——零唤醒源成因候选）。 */
            static _Atomic(int32_t) pend_log;
            if (atomic_fetch_add_explicit(&pend_log, 1,
                                          memory_order_relaxed) < 300) {
                fprintf(stderr,
                        "[pend] t=%p resume=%p canceled=%d bit=%u waker=%p pf=%u\n",
                        (void*)t, (void*)t->resume, t->canceled, bit,
                        (void*)t->waker, pf);
            }
            if (pf & RT_TASK_PF_POLLING) {
                out[7]++;
                /* POLLING 持有者详报：定位卡死 poll 的任务形态 */
                fprintf(stderr,
                        "[polling-task] t=%p st=%d bit=%u waker=%p resume=%p "
                        "poll_flags=%u from_slab=%d\n",
                        (void*)t, st, bit, (void*)t->waker, (void*)t->resume,
                        pf, t->from_slab);
            }
            else if (bit) {
                if (has_waker) {
                    out[1]++;
                    /* 唤醒链详报（限量）：PENDING+挂起+有 waker——waker 指向
                     * 唤醒者（outer 任务），配合 [nowk-task] 可画出等待图。 */
                    static _Atomic(int32_t) wk_log;
                    if (atomic_fetch_add_explicit(&wk_log, 1,
                                                  memory_order_relaxed) < 24) {
                        fprintf(stderr,
                                "[park-task] t=%p waker=%p resume=%p "
                                "from_slab=%d\n",
                                (void*)t, (void*)t->waker, (void*)t->resume,
                                t->from_slab);
                    }
                } else {
                    out[2]++;
                    /* 零唤醒源详报：PENDING+挂起等待+无 waker——没有任何机制会
                     * 唤醒它（卡点候选；round=65/round=11 现场 p_bit_nowk=2）。 */
                    fprintf(stderr,
                            "[nowk-task] t=%p resume=%p poll_flags=%u "
                            "from_slab=%d —— 零唤醒源任务\n",
                            (void*)t, (void*)t->resume, pf, t->from_slab);
                }
            }
            else if (t->resume) { if (has_waker) out[3]++; else out[4]++; }
            else {
                out[5]++;
                /* 壳任务详报（限量）：聚合器/Task.Run 壳——WhenAll outer 的
                 * 唤醒链核验。 */
                static _Atomic(int32_t) shell_log;
                if (atomic_fetch_add_explicit(&shell_log, 1,
                                              memory_order_relaxed) < 16) {
                    fprintf(stderr, "[shell-task] t=%p st=%d waker=%p\n",
                            (void*)t, st, (void*)t->waker);
                }
            }
        } else {
            out[6]++;
        }
        t = t->diag_next;
    }
    atomic_store_explicit(&g_live_lock, 0u, memory_order_release);
    /* 链完整性对账：lc（register/unregister 净计数）应等于遍历值 live。
     * lc > live → 链断（diag_prev/diag_next 被越界写坏——内存损坏直读证据，
     * 差值≈失踪任务数）；lc < live 不可能（遍历≤注册数，出现即头指针损坏）。 */
    {
        unsigned long long lc =
            (unsigned long long)atomic_load_explicit(&g_live_count,
                                                     memory_order_relaxed);
        if (lc != (unsigned long long)out[0]) {
            fprintf(stderr,
                    "[CHAIN-BROKEN] lc=%llu live_walk=%llu diff=%lld "
                    "—— 注册表链被写坏（内存损坏实锤），立即取证\n",
                    lc, (unsigned long long)out[0],
                    (long long)((long long)lc - (long long)out[0]));
        }
    }
}
#include <string.h>
#include <stdatomic.h>

#ifdef _WIN32
  #include <windows.h>
#else
  #include <pthread.h>
#endif

/* ---- RtTask 扩展字段 ----
 *
 * M5.2 在 RtTask 末尾追加 from_slab 标志（int32_t），标记分配来源。
 * slab 路径分配的 Task 释放时归还 free_list；malloc 路径释放时 free()。
 * next_free 仅在 free_list 中使用，复用已分配 Task 的 waker 槽位（8B）作为 next 指针。
 * 当 Task 在 free_list 中时，其 waker 字段无意义，可安全复用。
 */

static inline int rt_task_is_from_slab(RtTask* t) {
    return t->from_slab != 0;
}

static inline void rt_task_mark_from_slab(RtTask* t) {
    t->from_slab = 1;
}

static inline void rt_task_clear_slab_flag(RtTask* t) {
    t->from_slab = 0;
}

/* ---- Slab 结构 ----
 *
 * free_list 节点复用 RtTask 的 waker 槽位（rt_waker* waker 字段，8B）作为 next 指针。
 * 当 Task 在 free_list 中时，其 waker 字段无意义，可安全复用。
 *
 * cache padding: 64B 对齐避免多 worker 间 false sharing。
 */
typedef struct rt_task_slab {
    RtTask*  free_list;         /* 单链表头，owner 线程无锁访问 */
    int32_t  free_count;        /* free_list 当前节点数（cap 检查用，正确性必需） */
    int32_t  total_alloc;       /* 累计分配数（含 malloc fallback） */
    int32_t  _pad[13];          /* cache line 对齐 (64B) */
} rt_task_slab;

/* ---- TLS ----
 *
 * M5.2 热路径（RFC 009 M5 预算：10⁶ alloc/free <3ms）：每次 alloc/free 走
 * FlsGetValue/pthread_getspecific（~2-4ns/次）是热路径主成本之一。此处增加
 * __declspec(thread)/_Thread_local 快速指针（单条 mov 读 ~1ns），与 Fls/pthread_key
 * 保持同值：thread_init 时两处同步写，thread_destroy 时先清快速指针再触发 destructor
 *（destructor 仍在 thread 退出期运行，进程生命周期内无 UAF）。 */

#ifdef _WIN32
static __declspec(thread) rt_task_slab* g_slab_tls_fast = NULL;
static DWORD g_slab_tls = TLS_OUT_OF_INDEXES;
#else
static _Thread_local rt_task_slab* g_slab_tls_fast = NULL;
static pthread_key_t g_slab_tls;
static int           g_slab_tls_init = 0;
#endif

/* 懒初始化 TLS key 的一次性线程安全守卫（RFC 009 下钻 · 2026-08-07）。
 * 原 rt_task_slab_thread_init 由 worker/测试显式调用，bench 主线程从不调用 →
 * g_slab_tls_fast==NULL → 每 Task 走全局池 CAS。此处让 alloc/free 任意线程懒建
 * TLS slab（对齐 .NET gen0 bump / Go per-P），消除非 worker spawner 的 CAS 主路径。
 * g_slab_tls_key_ready 用 acquire 读、release 写，保证 key 可见后才被使用。 */
static _Atomic(int32_t) g_slab_tls_key_ready = 0;

/* 确保本线程拥有 TLS slab（无则懒建）。返回 1=可用，0=失败（回退全局池）。 */
static int rt_task_slab_ensure_thread(void) {
    if (g_slab_tls_fast) return 1;  /* 已建（快路径） */
    if (!atomic_load_explicit(&g_slab_tls_key_ready, memory_order_acquire)) {
        rt_task_slab_thread_init();  /* 首次：分配 TLS key + 建 slab */
    } else if (!g_slab_tls_fast) {
        /* key 已存在但本线程尚无 slab → 仅建 slab */
#ifdef _WIN32
        rt_task_slab* slab = (rt_task_slab*)calloc(1, sizeof(rt_task_slab));
        if (slab) {
            g_slab_tls_fast = slab;
            FlsSetValue(g_slab_tls, slab);
        }
#else
        rt_task_slab* slab = (rt_task_slab*)calloc(1, sizeof(rt_task_slab));
        if (slab) {
            g_slab_tls_fast = slab;
            pthread_setspecific(g_slab_tls, slab);
        }
#endif
    }
    return g_slab_tls_fast != NULL;
}

static void rt_task_slab_tls_destructor(void* p) {
    if (!p) return;
    rt_task_slab* slab = (rt_task_slab*)p;
    /* 释放 free_list 中所有 Task */
    RtTask* t = slab->free_list;
    while (t) {
        RtTask* next = (RtTask*)t->waker;
        free(t);
        t = next;
    }
    free(slab);
}

void rt_task_slab_thread_init(void) {
#ifdef _WIN32
    if (g_slab_tls == TLS_OUT_OF_INDEXES) {
        DWORD k = FlsAlloc(rt_task_slab_tls_destructor);
        if (k == TLS_OUT_OF_INDEXES) return;
        /* 一次性：仅首个成功者写入并发布 ready（并发懒建时其余线程各自 FlsAlloc
         * 会泄漏 key，但仅发生于多线程同时首次 alloc 的罕见启动窗口，可接受；
         * 已发布后 g_slab_tls_key_ready==1，后续线程不再进入）。 */
        g_slab_tls = k;
        atomic_store_explicit(&g_slab_tls_key_ready, 1, memory_order_release);
    }
    if (FlsGetValue(g_slab_tls)) return;  /* 已初始化 */
    rt_task_slab* slab = (rt_task_slab*)calloc(1, sizeof(rt_task_slab));
    if (slab) {
        FlsSetValue(g_slab_tls, slab);
        g_slab_tls_fast = slab;
    }
#else
    if (!g_slab_tls_init) {
        if (pthread_key_create(&g_slab_tls, rt_task_slab_tls_destructor) != 0) return;
        g_slab_tls_init = 1;
        atomic_store_explicit(&g_slab_tls_key_ready, 1, memory_order_release);
    }
    if (pthread_getspecific(g_slab_tls)) return;
    rt_task_slab* slab = (rt_task_slab*)calloc(1, sizeof(rt_task_slab));
    if (slab) {
        pthread_setspecific(g_slab_tls, slab);
        g_slab_tls_fast = slab;
    }
#endif
}

void rt_task_slab_thread_destroy(void) {
    /* 先清快速指针，再触发 destructor（FlsSetValue(NULL)/pthread_setspecific(NULL)）。 */
    g_slab_tls_fast = NULL;
#ifdef _WIN32
    if (g_slab_tls == TLS_OUT_OF_INDEXES) return;
    /* FlsSetValue(NULL) 触发 destructor */
    FlsSetValue(g_slab_tls, NULL);
#else
    if (!g_slab_tls_init) return;
    rt_task_slab_tls_destructor(pthread_getspecific(g_slab_tls));
    pthread_setspecific(g_slab_tls, NULL);
#endif
}

static rt_task_slab* rt_task_slab_current(void) {
    return g_slab_tls_fast;
}

/* ---- 全局 Task 池（RFC 009 §5 · 参照 Go sched.gFree / work_pool 同构）----
 *
 * 问题（原理分析 · 2026-08-07）：
 *   per-worker slab 仅 worker 线程有 TLS。Task.Run 从**任意线程**（主线程/EventLoop）
 *   spawn 时 g_slab_tls_fast==NULL → 每次 calloc(RtTask) ~80ns + free ~80ns。
 *   .NET Task 是 GC gen0 bump pointer（~10ns，无清零/无 malloc），故 Arc 的
 *   task_spawn_wait spawn 侧串行路径慢于 .NET —— 这是结构性主因，非调度器问题。
 *
 * 方案（对齐市场本质）：
 *   Go runtime 的 g 结构用双层池：per-P mcache/p.gFree（本地）+ sched.gFree（全局
 *   lock-free 栈）。Arc 的 work_pool 已是 Treiber stack，唯 Task 分配缺全局兜底。
 *   此处新增全局 lock-free free-list（Treiber + tag 防 ABA）：
 *     - worker 线程 → TLS slab（快路径不变）；
 *     - 非 worker 线程（主线程/EventLoop）→ 全局池 pop（~20-30ns，无 calloc）；
 *     - 释放：本地 slab 优先，无本地 slab 或 slab 满 → 全局池。
 *
 * 生命周期：全局池节点 = RtTask（复用 waker 槽位作 next，与 TLS slab free_list 同构）。
 * 池按 H1 纪律漏至进程退出（与 work_pool 同策，不触 CRT free）。
 * 释放路径已统一走 rt_task_slab_free（不按 from_slab 分支），from_slab 语义不变。 */
static _Atomic(uint64_t) g_task_pool_head  = 0;
static _Atomic(int32_t)  g_task_pool_count = 0;
#define RT_TASK_POOL_CAP 65536   /* 足够覆盖大批量 spawn 稳态复用（50000 任务零 calloc） */

#if UINTPTR_MAX > 0xFFFFFFFFu
#  define TASK_PTR_MASK 0x0000FFFFFFFFFFFFull
#  define TASK_TAG_MASK 0xFFFF000000000000ull
#  define TASK_TAG_ONE  0x0001000000000000ull
#else
#  define TASK_PTR_MASK 0xFFFFFFFFull
#  define TASK_TAG_MASK 0xFFFFFFFF00000000ull
#  define TASK_TAG_ONE  0x100000000ull
#endif

static RtTask* task_pool_pop(void) {
    uint64_t old = atomic_load_explicit(&g_task_pool_head, memory_order_relaxed);
    for (;;) {
        RtTask* t = (RtTask*)(uintptr_t)(old & TASK_PTR_MASK);
        if (!t) return NULL;
        RtTask* next = (RtTask*)(uintptr_t)((uint64_t)(uintptr_t)t->waker & TASK_PTR_MASK);
        uint64_t upd = ((old + TASK_TAG_ONE) & TASK_TAG_MASK) | ((uintptr_t)next & TASK_PTR_MASK);
        if (atomic_compare_exchange_weak_explicit(&g_task_pool_head, &old, upd,
                memory_order_acquire, memory_order_relaxed)) {
            atomic_fetch_sub_explicit(&g_task_pool_count, 1, memory_order_relaxed);
            return t;
        }
    }
}

static void task_pool_push(RtTask* t) {
    uint64_t old = atomic_load_explicit(&g_task_pool_head, memory_order_relaxed);
    for (;;) {
        t->waker = (rt_waker*)(uintptr_t)(old & TASK_PTR_MASK);
        uint64_t upd = (old & TASK_TAG_MASK) | ((uintptr_t)t & TASK_PTR_MASK);
        if (atomic_compare_exchange_weak_explicit(&g_task_pool_head, &old, upd,
                memory_order_release, memory_order_relaxed)) {
            atomic_fetch_add_explicit(&g_task_pool_count, 1, memory_order_relaxed);
            return;
        }
    }
}

/* ---- Slab API ---- */

/* free_list 容量上限：避免长运行程序无界内存增长。
 * 超过 cap 的释放直接 free()。1024 足以覆盖热点 Task 复用。 */
#define RT_TASK_SLAB_CAP 1024

/* 清零前缀长度（A3 收敛 · RFC 016 §1.2 可达地板）：仅覆盖「释放/查询路径依赖为 0」
 * 的正确性关键字段。RtTask 布局（96B，§3.5 冻结面不可改布局）：status(0)/
 * canceled(4)/int_result(8)/ptr_result(16)/value_result(24)/value_size(32)/
 * resume(40)/resume_data(48)/waker(56)/_waker_slot(64)/from_slab(80)/dtor_fn(88)。
 *
 * 前缀 32B 覆盖（offset 0..31，offsetof(RtTask, value_size)）：
 *   - canceled（4）：rt_task_poll/status 读它；回收脏值 1 会让新 Task 恒判取消；
 *   - int_result（8）/ptr_result（16）/value_result（24）：结果槽，codegen 在
 *     写入前读取的路径依赖零基线（Task.Run trampoline 等写结果先于 complete）。
 *
 * 可留脏（各字段安全论证）：
 *   - value_size（32）：仅当 value_result!=NULL 时经 rt_task_result_value 读取；
 *     release 路径总是先 free 再置 value_result=NULL（rt_task.c L45-48），回收时必 NULL；
 *   - resume（40）：仅 status==PENDING 时被 poll 读取；下方 alloc 显式置 NULL——
 *     Task.Run 直接置 PENDING 且不设 resume（rt_task_run.c），依赖 resume==NULL 挡 poll；
 *   - resume_data（48）：release 总是 free+置 NULL（rt_task.c L50-57）；仅经 resume 读取；
 *   - waker（56）：free_list 复用槽位；下方 alloc 显式置 NULL——poll resume 完成路径
 *     （rt_task.c L94-95）与 rt_task_complete（L244-247）读它，回收脏值会以垃圾指针
 *     触发 wake → UAF；
 *   - _waker_slot（64）：仅在 waker!=NULL 时经指针读取；register_waker 使用前总是
 *     完整写入 _waker_slot.data/.wake（rt_task.c L276-286）；
 *   - from_slab（80）/dtor_fn（88）：下方显式设置。
 *
 * 相比原 offsetof(RtTask, from_slab)=80B 减少 48B 写入（RFC 009 M5 预算热路径）。 */
#define RT_TASK_ZERO_PREFIX offsetof(RtTask, value_size)  /* 32B */

/* TLS slab ↔ 全局池批量搬运粒度（RFC 009 下钻 · 2026-08-07）。
 * 原先 TLS slab 空时每 Task 一次 task_pool_pop 全局 CAS（主线程 spawn 热路径）。
 * 现批量从全局池搬运 RT_TASK_SLAB_BATCH 个到 TLS，1 次 CAS 摊销 BATCH 次 alloc。 */
#define RT_TASK_SLAB_BATCH 64

/* 从全局池批量搬运填充 TLS slab（空时调用）。 */
static void rt_task_slab_refill(rt_task_slab* slab) {
    for (int32_t i = 0; i < RT_TASK_SLAB_BATCH && slab->free_count < RT_TASK_SLAB_CAP; i++) {
        RtTask* t = task_pool_pop();
        if (!t) break;
        t->waker = (rt_waker*)slab->free_list;
        slab->free_list = t;
        slab->free_count++;
    }
}

/* 从当前线程的 slab 分配 RtTask。空则批量填充，全局池也空则 calloc fallback。
 * 返回的 Task 已清零，from_slab 标志已设置（1=slab 来源，0=malloc 来源）。
 * 调用方负责设置 status/resume 等业务字段。
 *
 * RFC 009 下钻（2026-08-07）：任意线程（含非 worker 主线程/LSP/EventLoop）懒建
 * TLS slab，消除 spawner 每 Task 的全局池 CAS——对齐 .NET gen0 bump / Go per-P。 */
RtTask* rt_task_slab_alloc(void) {
    rt_task_slab* slab = rt_task_slab_current();
    if (!slab) {
        if (!rt_task_slab_ensure_thread()) {
            /* 懒建失败（极端）：回退全局池单 pop */
            RtTask* t = task_pool_pop();
            if (t) {
                memset(t, 0, RT_TASK_ZERO_PREFIX);
                rt_task_mark_from_slab(t);
                t->dtor_fn = NULL;
                t->status = RT_TASK_READY;
                t->resume = NULL;
                t->waker = NULL;
                t->follower_head = NULL;
                t->follower_next = NULL;
                t->diag_prev = NULL;
                t->diag_next = NULL;
                t->diag_linked = 0;
                t->diag_freed = 0;
                rt_live_register(t);
                return t;
            }
            t = (RtTask*)calloc(1, sizeof(RtTask));
            if (t) { t->status = RT_TASK_READY; rt_live_register(t); }
            return t;
        }
        slab = rt_task_slab_current();
    }
    if (!slab->free_list) rt_task_slab_refill(slab);
    if (slab->free_list) {
        RtTask* t = slab->free_list;
        slab->free_list = (RtTask*)t->waker;
        slab->free_count--;
        /* 清零正确性关键字段（32B 前缀）。waker/resume 可能残留自上一生命周期
         *（resume 完成路径不清理 waker），必须显式置 NULL——它们不在前缀内。
         * follower 链同理（RFC 008）：残留脏值会让 release 误判扇出链非空
         * → fire 垃圾指针 → 堆损坏。 */
        memset(t, 0, RT_TASK_ZERO_PREFIX);
        rt_task_mark_from_slab(t);
        t->dtor_fn = NULL;
        t->status = RT_TASK_READY;
        t->resume = NULL;
        t->waker = NULL;
        t->follower_head = NULL;
        t->follower_next = NULL;
        /* diag 注册表字段不在 ZERO_PREFIX 内：残留脏值会让 register 幂等
         * 保护误拦（diag_linked=1）或摘链写坏链（diag_prev/next 指向已
         * 复用任务）——复用时一并归零。 */
        t->diag_prev = NULL;
        t->diag_next = NULL;
        t->diag_linked = 0;
        t->diag_freed = 0;
        rt_live_register(t);
        return t;
    }
    /* 全局池也空 → calloc（首次/极端场景） */
    rt_task_slab* s = rt_task_slab_current();
    if (s) s->total_alloc++;
    RtTask* t = (RtTask*)calloc(1, sizeof(RtTask));
    if (t) { t->status = RT_TASK_READY; rt_live_register(t); }
    /* from_slab = 0 (calloc 已清零) */
    return t;
}

/* 将 Task 归还当前线程的 slab free_list。
 * 所有来源的 Task（malloc 或 slab）都优先推入 free_list 以实现稳态零 malloc；
 * 仅当 free_list 已达 cap 上限时才进入全局池/free()。
 * 调用方需确保 Task 已完成且不再被引用。
 *
 * RFC 009 下钻（2026-08-07）：无 TLS slab 的线程懒建（对齐 alloc 侧），
 * 使非 worker 释放路径也走本地 slab（O(1)），仅 slab 满时批量流到全局池。 */
void rt_task_slab_free(RtTask* t) {
    if (!t) return;
    if (t->diag_freed) {
        /* 已归还过：第二次 free 即 double-free（所有权协议违例）。
         * 若照常入池会造成 free_list/全局池链环 + 同一任务双重分配，
         * 是历史内存损坏族的头号嫌疑。跳过归还 + 打栈定位双发两方。 */
        fprintf(stderr, "[DOUBLE-FREE] t=%p status=%d from_slab=%d "
                        "—— 任务二次归还内存池\n",
                (void*)t, atomic_load_explicit((_Atomic int32_t*)&t->status,
                                               memory_order_relaxed),
                t->from_slab);
        rt_diag_unreg_btrace();
        return;
    }
    t->diag_freed = 1;
    rt_live_unregister(t);
    rt_task_slab* slab = rt_task_slab_current();
    if (!slab) {
        if (!rt_task_slab_ensure_thread()) {
            /* 懒建失败（极端）：直接全局池 push */
            task_pool_push(t);
            return;
        }
        slab = rt_task_slab_current();
    }
    if (slab->free_count < RT_TASK_SLAB_CAP) {
        /* 推入本地 free_list（复用 waker 槽位作为 next 指针） */
        t->waker = (rt_waker*)slab->free_list;
        slab->free_list = t;
        slab->free_count++;
        return;
    }
    /* 本地 slab 已达 cap，全局池未满 → 全局池兜底 */
    if (atomic_load_explicit(&g_task_pool_count, memory_order_relaxed) < RT_TASK_POOL_CAP) {
        task_pool_push(t);
        return;
    }
    /* 全局池也满 → 直接 free */
    free(t);
}

int32_t rt_task_slab_free_count(void) {
    rt_task_slab* slab = rt_task_slab_current();
    return slab ? slab->free_count : 0;
}

int32_t rt_task_slab_in_use(void) {
    /* A3 收敛（RFC 016 §1.2）：热路径已移除 in_use 计数（非正确性必需，仅监控）。
     * 本 ABI 签名保留（冻结面），恒返回 0——监控语义由 free_count/total_alloc 承担。 */
    return 0;
}

/* 注册表对账探针：lc=register/unregister 净计数，pool=全局池 free 节点数。
 * 与 census 遍历值 live 三方对照可定性「任务消失」根因：
 *   lc>0 且 live=0 → 注册表链断（内存损坏实锤，差值≈断链规模）；
 *   lc<0 → double-unregister（生命周期协议违例）；
 *   lc=0 且 live=0 → 任务确实全部走完 unregister（生命周期路径问题）。 */
uint64_t rt_diag_live_count(void) {
    return (uint64_t)atomic_load_explicit(&g_live_count, memory_order_relaxed);
}

int32_t rt_diag_pool_count(void) {
    return atomic_load_explicit(&g_task_pool_count, memory_order_relaxed);
}

int32_t rt_task_slab_total_alloc(void) {
    rt_task_slab* slab = rt_task_slab_current();
    return slab ? slab->total_alloc : 0;
}
