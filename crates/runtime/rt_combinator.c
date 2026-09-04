// Task combinator ABI (RFC 009 M4).
//
// WhenAll/WhenAny 真实异步组合子：从 M1 同步占位（rt_task_void()）升级为
// runtime 层 aggregator + per-inner waker binding。
//
// 核心设计：
//   - aggregator：N 个 inner 共享一个 aggregator，记录 remaining（WhenAll）
//     或 any_completed（WhenAny）。最后一个完成的 inner 唤醒 outer。
//   - per-inner binding：每个 inner 独立 waker 槽（不复用 RtTask._waker_slot 单槽）。
//     binding 的 waker_slot.wake 回调 rt_when_{all,any}_inner_complete。
//   - waker 注册：直接设置 inner->waker = &binding->waker_slot（waker 是指针字段）。
//
// 边界情况：
//   - count==0 → outer 立即 READY
//   - 所有 inner 已完成 → outer 立即 READY
//
// M4 单线程 MVP：mutex 用于 M5 多线程兼容（防护性锁）。

#include "rt_abi.h"
#include <stdatomic.h>
#include <stdlib.h>
#include <string.h>

/* ---- 平台抽象（与 rt_cts.c/rt_event_loop.c 重复，M5 时统一到 rt_platform.h） ---- */

#ifdef _WIN32
  #include <windows.h>
  typedef CRITICAL_SECTION     rt_comb_mutex_t;
#else
  #include <pthread.h>
  typedef pthread_mutex_t      rt_comb_mutex_t;
#endif

static void rt_comb_mutex_init(rt_comb_mutex_t* m) {
#ifdef _WIN32
    InitializeCriticalSection(m);
#else
    pthread_mutex_init(m, NULL);
#endif
}
static void rt_comb_mutex_destroy(rt_comb_mutex_t* m) {
#ifdef _WIN32
    DeleteCriticalSection(m);
#else
    pthread_mutex_destroy(m);
#endif
}
static void rt_comb_mutex_lock(rt_comb_mutex_t* m) {
#ifdef _WIN32
    EnterCriticalSection(m);
#else
    pthread_mutex_lock(m);
#endif
}
static void rt_comb_mutex_unlock(rt_comb_mutex_t* m) {
#ifdef _WIN32
    LeaveCriticalSection(m);
#else
    pthread_mutex_unlock(m);
#endif
}

/* ---- 聚合器与 binding ---- */

typedef struct RtCombinatorAggregator {
    void*           outer_task;    /* await WhenAll/WhenAny 的 outer Task */
    int32_t         remaining;     /* 未完成 inner 数（初值 count，单调递减，禁覆写） */
    int32_t         any_completed; /* WhenAny：0/1（首个完成的标记为 1） */
    int32_t         is_when_all;   /* 1=WhenAll, 0=WhenAny */
    int32_t         fired;         /* WhenAll：outer 已 fire 标记（幂等，防二次 complete） */
    int32_t         binding_count; /* binding 数组长度（用于释放） */
    rt_comb_mutex_t mutex;
} RtCombinatorAggregator;

typedef struct RtCombinatorBinding {
    RtCombinatorAggregator* agg;
    rt_waker waker_slot;           /* 内嵌 waker：wake 回调由 is_when_all 决定 */
} RtCombinatorBinding;

/* 完成计数（扫描侧与回调侧共用）：remaining 单调递减至 0 触发 outer。
 * 旧实现两处丢失唤醒（case8 bounded_mpmc_stress，2026-09-03 根治）：
 *   1. 登记竞态——create 扫描循环 `poll(inner) → 安装 binding waker` 两步间
 *      inner 可并发完成：complete 见 waker NULL 不触发，binding 装在已终态
 *      任务上永不 fire → remaining 永不归零 → outer 永挂。修复：安装改经
 *      rt_task_wk_lock 临界区（锁内复检 status，终态则不装走计数路径）。
 *   2. 覆写竞态——扫描期已装 binding 的 inner 完成即 fire（remaining--），
 *      而扫描尾 `remaining = count - already_done` 无条件覆写回涂该次递减
 *      → 终值恒 ≥1 → 永挂。修复：remaining 只减不覆写。 */
static void rt_comb_dec_completed(RtCombinatorAggregator* agg) {
    int32_t need_fire = 0;
    int32_t need_free = 0;
    rt_comb_mutex_lock(&agg->mutex);
    if (agg->is_when_all) {
        if (agg->remaining > 0) agg->remaining--;
        if (agg->remaining == 0 && !agg->fired) {
            agg->fired = 1;
            need_fire = 1;
            need_free = 1; /* WhenAll：归零即全部完成，此后无回调可触发 */
        }
    } else {
        if (!agg->any_completed) {
            agg->any_completed = 1;
            need_fire = 1;
        }
        agg->remaining--;
        if (agg->remaining <= 0) need_free = 1;
    }
    rt_comb_mutex_unlock(&agg->mutex);
    if (need_fire) {
        rt_task_complete(agg->outer_task);
    }
    if (need_free) {
        rt_comb_mutex_destroy(&agg->mutex);
        free(agg); /* agg + bindings 一次连续分配，free(agg) 释放全部 */
    }
}

/* WhenAll inner 完成回调：dec remaining；归零时唤醒 outer + 释放 aggregator */
static void rt_when_all_inner_complete(void* data) {
    RtCombinatorBinding* binding = (RtCombinatorBinding*)data;
    if (!binding || !binding->agg) return;
    rt_comb_dec_completed(binding->agg);
}

/* WhenAny inner 完成回调：首个完成的 inner 唤醒 outer。
 * 其余 Pending inner 仍可能稍后 fire waker——须等 remaining 耗尽再 free，
 * 否则对已释放 binding 二次 wake（UAF）。 */
static void rt_when_any_inner_complete(void* data) {
    RtCombinatorBinding* binding = (RtCombinatorBinding*)data;
    if (!binding || !binding->agg) return;
    rt_comb_dec_completed(binding->agg);
}

/* ---- 公共 ABI ---- */

/* 通用构造：创建 outer Task + aggregator + N 个 binding，注册 waker 到未完成 inner。
 * is_when_all=1 → WhenAll 语义；is_when_all=0 → WhenAny 语义。 */
static void* rt_task_combinator_create(void** tasks, int32_t count, int32_t is_when_all) {
    /* 创建 outer Task（Pending，resume=NULL，靠 waker 唤醒转 READY）。
     * slab 分配（自动注册活任务表 + 复用）：裸 calloc 任务链外身份会让其首次
     * 合法释放误触 [DOUBLE-UNREG]，且普查漏计聚合任务。 */
    RtTask* outer = (RtTask*)rt_task_slab_alloc();
    if (!outer) return rt_task_void();
    outer->status = RT_TASK_PENDING;
    outer->canceled = 0;
    outer->resume = NULL;
    outer->resume_data = NULL;
    outer->waker = NULL;

    if (count <= 0) {
        /* 空数组：outer 立即 READY */
        outer->status = RT_TASK_READY;
        return outer;
    }

    /* 一次 malloc 分配 aggregator + N 个 binding（连续内存，一次 free） */
    size_t total = sizeof(RtCombinatorAggregator) + (size_t)count * sizeof(RtCombinatorBinding);
    char* block = (char*)calloc(1, total);
    if (!block) {
        outer->status = RT_TASK_READY;
        return outer;
    }
    RtCombinatorAggregator* agg = (RtCombinatorAggregator*)block;
    RtCombinatorBinding* bindings = (RtCombinatorBinding*)(block + sizeof(RtCombinatorAggregator));

    agg->outer_task = outer;
    agg->remaining = count;
    agg->any_completed = 0;
    agg->is_when_all = is_when_all;
    agg->fired = 0;
    agg->binding_count = count;
    rt_comb_mutex_init(&agg->mutex);

    /* 初始化每个 binding 的 waker_slot */
    for (int32_t i = 0; i < count; i++) {
        bindings[i].agg = agg;
        bindings[i].waker_slot.data = &bindings[i];
        bindings[i].waker_slot.wake = is_when_all
            ? rt_when_all_inner_complete
            : rt_when_any_inner_complete;
    }

    /* 遍历 inner：锁内复检状态，终态走计数路径；Pending 在锁内安装 binding
     * waker（与 complete/fault 的 snapshot 互斥，安装后完成必触发回调）。 */
    for (int32_t i = 0; i < count; i++) {
        if (!tasks[i]) {
            /* NULL 视为已完成（计数路径；旧实现 continue 会把它留在
             * remaining 里致永挂） */
            rt_comb_dec_completed(agg);
            continue;
        }
        RtTask* inner = (RtTask*)tasks[i];
        rt_task_poll(inner);
        rt_task_wk_lock(inner);
        int32_t status = atomic_load_explicit((_Atomic(int32_t)*)&inner->status,
                                              memory_order_acquire);
        if (status == RT_TASK_READY || status == RT_TASK_FAULTED) {
            rt_task_wk_unlock(inner);
            rt_comb_dec_completed(agg);
        } else {
            /* Pending：锁内安装 binding waker（旧实现锁外裸装——与 complete
             * 的 snapshot 竞态即丢失唤醒 hole 1） */
            inner->waker = &bindings[i].waker_slot;
            rt_task_wk_unlock(inner);
        }
    }

    /* outer 保持 Pending（全终态时已由计数路径 fire 为 READY） */
    return outer;
}

void* rt_task_when_all(void** tasks, int32_t count) {
    return rt_task_combinator_create(tasks, count, /*is_when_all=*/1);
}

void* rt_task_when_any(void** tasks, int32_t count) {
    return rt_task_combinator_create(tasks, count, /*is_when_all=*/0);
}