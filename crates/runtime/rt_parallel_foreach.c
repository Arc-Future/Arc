// Parallel.ForEach runtime implementation (RFC 009 M6).
//
// 将数组源 [0, len) 分区，分发到 ThreadPool 并行执行 body(i, &array[i], env)，
// 阻塞直到所有分区完成。复用 rt_parallel_for 的分区 + completion 原语策略。
//
// ## 设计要点
//
// - 分区策略：自动计算 partition_size = max(1, total / (n_workers * 4))
// - 元素访问：通过 RtArrayHeader.elem_size 计算 &array[i] = base + i * elem_size
//   （elem_size 从数组 header 读取，codegen 无需传递）
// - 取消支持：cts 取消时 body 检查 rt_cts_is_canceled 提前退出（MVP 预留）
// - 线程安全：body 闭包在 worker 线程上执行；env 需由调用方保证线程安全
//
// ## 多平台说明
//
// 平台无关的分区调度逻辑；底层线程池/信号量由 rt_threadpool 提供
// （Windows 用 Semaphore，Linux/macOS 用 sem_t）。元素指针计算通过
// 字节偏移，支持任意元素类型与对齐。
//
// ## 当前限制（M6 MVP）
//
// body 签名为 void(*)(int32_t index, void* elem_ptr, void* env)——传递元素指针。
// 对引用类型（class/string/array）完全正确（T 本身是指针）。
// 对值类型（int/float/struct），codegen 生成的 trampoline 需做类型感知 load，
// 当前 MVP 仅传递指针——值类型 ForEach 建议使用 Parallel.For + 手动索引。

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

#define RT_FOREACH_MAX_PARTITIONS 1024

/* Arc 数组 header（与 rt_array.c 中 RtArrayHeader 一致）。
 * payload 指针 = header + 8；elem_size 在 header 偏移 4 处。 */
typedef struct {
    int32_t length;
    int32_t elem_size;
} rt_foreach_array_header;

static int32_t rt_foreach_read_elem_size(void* array_ptr) {
    if (!array_ptr) return 0;
    rt_foreach_array_header* h = (rt_foreach_array_header*)((char*)array_ptr - sizeof(rt_foreach_array_header));
    return h->elem_size;
}

/* 分区元数据 */
typedef struct rt_foreach_range {
    int32_t  from;
    int32_t  to;
} rt_foreach_range;

/* 工作项 payload：每个分区的上下文 */
typedef struct rt_foreach_work {
    void*           array_ptr;    /* 数组首元素指针（payload，header 之后） */
    int32_t         elem_size;    /* 元素大小（字节，从 header 读取） */
    void          (*body)(int32_t index, void* elem_ptr, void* env);
    void*           env;
    _Atomic(int32_t)* done_counter;
#ifdef _WIN32
    HANDLE          done_semaphore;
#else
    sem_t*          done_semaphore;
#endif
} rt_foreach_work;

/* 分区工作 trampoline：遍历 [from, to) 调用 body(i, &array[i], env) */
static void rt_foreach_range_worker(void* raw) {
    struct {
        rt_foreach_range range;
        rt_foreach_work  work;
    } *payload = (void*)raw;

    char* base = (char*)payload->work.array_ptr;
    int32_t elem_size = payload->work.elem_size;

    for (int32_t i = payload->range.from; i < payload->range.to; i++) {
        void* elem_ptr = base + (size_t)i * (size_t)elem_size;
        payload->work.body(i, elem_ptr, payload->work.env);
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

int32_t rt_parallel_foreach(void* array_ptr,
                            int32_t array_len,
                            void (*body)(int32_t index, void* elem_ptr, void* env),
                            void* env,
                            rt_threadpool* pool,
                            void* cts_ignored,
                            int32_t max_degree) {
    (void)cts_ignored;  /* MVP 不使用 CTS */
    if (!array_ptr || array_len <= 0 || !body) return 0;

    /* 从数组 header 读取 elem_size（codegen 无需传递） */
    int32_t elem_size = rt_foreach_read_elem_size(array_ptr);
    if (elem_size <= 0) elem_size = 8;  /* 降级：指针大小 */

    int32_t total = array_len;

    /* 确定分区数和每分区大小 */
    int32_t n_workers = pool ? rt_threadpool_worker_count(pool) : 1;
    if (n_workers <= 0) n_workers = 1;
    if (max_degree > 0 && max_degree < n_workers) n_workers = max_degree;

    /* 分区大小：total / (workers * 4)，至少 1 */
    int32_t partition_size = total / (n_workers * 4);
    if (partition_size < 1) partition_size = 1;

    int32_t n_partitions = (total + partition_size - 1) / partition_size;
    if (n_partitions > RT_FOREACH_MAX_PARTITIONS) {
        n_partitions = RT_FOREACH_MAX_PARTITIONS;
        partition_size = (total + n_partitions - 1) / n_partitions;
    }

    /* completion 原语 */
    _Atomic int32_t done_counter = 0;
#ifdef _WIN32
    HANDLE done_sem = CreateSemaphoreA(NULL, 0, RT_FOREACH_MAX_PARTITIONS, NULL);
#else
    sem_t done_sem;
    sem_init(&done_sem, 0, 0);
#endif

    rt_foreach_work shared_work;
    shared_work.array_ptr = array_ptr;
    shared_work.elem_size = elem_size;
    shared_work.body = body;
    shared_work.env = env;
    shared_work.done_counter = &done_counter;
#ifdef _WIN32
    shared_work.done_semaphore = done_sem;
#else
    shared_work.done_semaphore = &done_sem;
#endif

    /* 分区并分发 */
    int32_t current = 0;
    for (int32_t p = 0; p < n_partitions; p++) {
        int32_t range_from = current;
        int32_t range_to = range_from + partition_size;
        if (range_to > total) range_to = total;
        current = range_to;

        /* 分配 payload */
        struct {
            rt_foreach_range range;
            rt_foreach_work  work;
        } *payload = (void*)malloc(sizeof(*payload));
        if (!payload) break;

        payload->range.from = range_from;
        payload->range.to = range_to;
        payload->work = shared_work;

        rt_work_t w;
        w.fn = rt_foreach_range_worker;
        w.data = payload;

        if (pool) {
            rt_threadpool_spawn(pool, w);
        } else {
            /* 无线程池：同步执行 */
            rt_foreach_range_worker(payload);
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
