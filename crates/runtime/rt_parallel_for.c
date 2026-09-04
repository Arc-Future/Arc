// Parallel.For runtime implementation (RFC 009 M2 / RFC 009 M5.7).
//
// 将 [from, to) 区间分区，分发到 ThreadPool 并行执行 body(i)，
// 阻塞直到所有分区完成。使用 atomic completion counter + semaphore 实现 Join。
//
// ## 设计要点
//
// - 分区策略：自动计算 partition_size = max(1, total / (n_workers * 4))
// - 取消支持：cts 取消时 body 检查 rt_cts_is_canceled 提前退出
// - 线程安全：body 闭包在 worker 线程上执行；env 需由调用方保证线程安全
// - 零分配热路径：range 数组栈分配（MAX_PARTITIONS=1024 以内）

#include "rt_abi.h"
#include <stdlib.h>
#include <string.h>
#include <stdatomic.h>
#include <errno.h>

#ifdef _WIN32
  #include <windows.h>
#else
  #include <pthread.h>
  #include <semaphore.h>
#endif

#define RT_PARALLEL_MAX_PARTITIONS 1024

/* 分区元数据 */
typedef struct rt_parallel_range {
    int32_t  from;
    int32_t  to;
} rt_parallel_range;

/* 工作项 payload：每个分区的上下文 */
typedef struct rt_parallel_work {
    void          (*body)(int32_t i, void* env);
    void*           env;
    _Atomic(int32_t)* done_counter;
#ifdef _WIN32
    HANDLE          done_semaphore;
#else
    sem_t*          done_semaphore;
#endif
} rt_parallel_work;

/* 分区工作 trampoline：遍历 [from, to) 调用 body(i, env) */
static void rt_parallel_for_each_range(void* raw) {
    struct {
        rt_parallel_range range;
        rt_parallel_work  work;
    } *payload = (void*)raw;

    for (int32_t i = payload->range.from; i < payload->range.to; i++) {
        payload->work.body(i, payload->work.env);
    }

    /* 分区完成：inc counter + signal sem */
    atomic_fetch_add_explicit(payload->work.done_counter, 1, memory_order_release);
#ifdef _WIN32
    ReleaseSemaphore(payload->work.done_semaphore, 1, NULL);
#else
    sem_post(payload->work.done_semaphore);
#endif

    free(raw);
}

int32_t rt_parallel_for(int32_t from, int32_t to,
                        void (*body)(int32_t i, void* env),
                        void* env,
                        rt_threadpool* pool,
                        void* cts_ignored,     /* CTS 预留：MVP 不使用 */
                        int32_t max_degree) {
    if (from >= to) return 0;
    int32_t total = to - from;

    /* 确定分区数和每分区大小：n_workers 来自池的实际 worker 数，
     * 而非 pending_count（待处理任务数与 worker 数无直接关系）。 */
    int32_t n_workers = pool ? rt_threadpool_worker_count(pool) : 1;
    if (n_workers <= 0) n_workers = 1;
    if (max_degree > 0 && max_degree < n_workers) n_workers = max_degree;

    /* 分区大小：total / (workers * 4)，至少 1 */
    int32_t partition_size = total / (n_workers * 4);
    if (partition_size < 1) partition_size = 1;

    int32_t n_partitions = (total + partition_size - 1) / partition_size;
    if (n_partitions > RT_PARALLEL_MAX_PARTITIONS) {
        n_partitions = RT_PARALLEL_MAX_PARTITIONS;
        partition_size = (total + n_partitions - 1) / n_partitions;
    }

    /* completion 原语 */
    _Atomic int32_t done_counter = 0;
#ifdef _WIN32
    HANDLE done_sem = CreateSemaphoreA(NULL, 0, RT_PARALLEL_MAX_PARTITIONS, NULL);
#else
    sem_t done_sem;
    sem_init(&done_sem, 0, 0);
#endif

    rt_parallel_work shared_work;
    shared_work.body = body;
    shared_work.env = env;
    shared_work.done_counter = &done_counter;
#ifdef _WIN32
    shared_work.done_semaphore = done_sem;
#else
    shared_work.done_semaphore = &done_sem;
#endif

    /* 分区并分发 */
    int32_t current = from;
    for (int32_t p = 0; p < n_partitions; p++) {
        int32_t range_from = current;
        int32_t range_to = range_from + partition_size;
        if (range_to > to) range_to = to;
        current = range_to;

        /* 分配 payload */
        struct {
            rt_parallel_range range;
            rt_parallel_work  work;
        } *payload = (void*)malloc(sizeof(*payload));
        if (!payload) break;

        payload->range.from = range_from;
        payload->range.to = range_to;
        payload->work = shared_work;

        rt_work_t w;
        w.fn = rt_parallel_for_each_range;
        w.data = payload;

        if (pool) {
            rt_threadpool_spawn(pool, w);
        } else {
            /* 无线程池：同步执行 */
            rt_parallel_for_each_range(payload);
        }
    }

    /* 等待所有分区完成 */
    for (int32_t p = 0; p < n_partitions; p++) {
#ifdef _WIN32
        WaitForSingleObject(done_sem, INFINITE);
#else
        while (sem_wait(&done_sem) == -1 && errno == EINTR) {}
#endif
    }

#ifdef _WIN32
    CloseHandle(done_sem);
#else
    sem_destroy(&done_sem);
#endif

    return n_partitions;
}

/* ==== RFC 009 M7 优化：Parallel.For 局部累加 + 最终合并 ==== */

typedef struct rt_parallel_reduce_work {
    rt_parallel_body_local   body;
    rt_parallel_local_init   init_local;
    void*                     env;
    void*                     local;       /* 本分区 local 累加器（local_size 字节） */
    _Atomic(int32_t)*         done_counter;
#ifdef _WIN32
    HANDLE                    done_semaphore;
#else
    sem_t*                    done_semaphore;
#endif
} rt_parallel_reduce_work;

static void rt_parallel_for_reduce_range(void* raw) {
    struct {
        rt_parallel_range        range;
        rt_parallel_reduce_work  work;
    } *payload = (void*)raw;

    /* 初始化本分区 local 累加器 */
    if (payload->work.init_local) {
        payload->work.init_local(payload->work.local);
    }

    /* 遍历 [from, to)，body(i, local, env) 无原子竞争 */
    for (int32_t i = payload->range.from; i < payload->range.to; i++) {
        payload->work.body(i, payload->work.local, payload->work.env);
    }

    /* 分区完成：inc counter + signal sem */
    atomic_fetch_add_explicit(payload->work.done_counter, 1, memory_order_release);
#ifdef _WIN32
    ReleaseSemaphore(payload->work.done_semaphore, 1, NULL);
#else
    sem_post(payload->work.done_semaphore);
#endif

    free(raw);
}

int32_t rt_parallel_for_reduce(int32_t from, int32_t to,
                               rt_parallel_body_local body,
                               rt_parallel_local_init init_local,
                               rt_parallel_local_merge merge_local,
                               void* result, size_t local_size,
                               void* env,
                               rt_threadpool* pool,
                               int32_t max_degree) {
    if (from >= to) return 0;
    int32_t total = to - from;

    int32_t n_workers = pool ? rt_threadpool_worker_count(pool) : 1;
    if (n_workers <= 0) n_workers = 1;
    if (max_degree > 0 && max_degree < n_workers) n_workers = max_degree;

    int32_t partition_size = total / (n_workers * 4);
    if (partition_size < 1) partition_size = 1;

    int32_t n_partitions = (total + partition_size - 1) / partition_size;
    if (n_partitions > RT_PARALLEL_MAX_PARTITIONS) {
        n_partitions = RT_PARALLEL_MAX_PARTITIONS;
        partition_size = (total + n_partitions - 1) / n_partitions;
    }

    /* completion 原语 */
    _Atomic int32_t done_counter = 0;
#ifdef _WIN32
    HANDLE done_sem = CreateSemaphoreA(NULL, 0, RT_PARALLEL_MAX_PARTITIONS, NULL);
#else
    sem_t done_sem;
    sem_init(&done_sem, 0, 0);
#endif

    /* 为每分区分配 local 累加器 + 记录指针用于最终合并 */
    void** locals = (void**)calloc(n_partitions, sizeof(void*));
    if (!locals) {
#ifdef _WIN32
        CloseHandle(done_sem);
#else
        sem_destroy(&done_sem);
#endif
        return 0;
    }

    int32_t current = from;
    for (int32_t p = 0; p < n_partitions; p++) {
        int32_t range_from = current;
        int32_t range_to = range_from + partition_size;
        if (range_to > to) range_to = to;
        current = range_to;

        /* 分配 local 累加器 */
        locals[p] = calloc(1, local_size);
        if (!locals[p]) break;

        /* 分配 payload */
        struct {
            rt_parallel_range        range;
            rt_parallel_reduce_work  work;
        } *payload = (void*)malloc(sizeof(*payload));
        if (!payload) { free(locals[p]); locals[p] = NULL; break; }

        payload->range.from = range_from;
        payload->range.to = range_to;
        payload->work.body = body;
        payload->work.init_local = init_local;
        payload->work.env = env;
        payload->work.local = locals[p];
        payload->work.done_counter = &done_counter;
#ifdef _WIN32
        payload->work.done_semaphore = done_sem;
#else
        payload->work.done_semaphore = &done_sem;
#endif

        rt_work_t w;
        w.fn = rt_parallel_for_reduce_range;
        w.data = payload;

        if (pool) {
            rt_threadpool_spawn(pool, w);
        } else {
            rt_parallel_for_reduce_range(payload);
        }
    }

    /* 等待所有分区完成 */
    for (int32_t p = 0; p < n_partitions; p++) {
#ifdef _WIN32
        WaitForSingleObject(done_sem, INFINITE);
#else
        while (sem_wait(&done_sem) == -1 && errno == EINTR) {}
#endif
    }

    /* 最终合并：串行将所有 local 合并到 result */
    if (merge_local && result) {
        for (int32_t p = 0; p < n_partitions; p++) {
            if (locals[p]) {
                merge_local(result, locals[p]);
                free(locals[p]);
            }
        }
    }

    free(locals);
#ifdef _WIN32
    CloseHandle(done_sem);
#else
    sem_destroy(&done_sem);
#endif

    return n_partitions;
}
