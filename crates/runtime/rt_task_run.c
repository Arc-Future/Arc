// Task.Run ABI (RFC 009 M5.7).
// 将 Action/Func<T> 调度到 ThreadPool 并返回 Task 句柄。
// 依赖 rt_task.c（Task 生命周期）+ rt_threadpool.c（线程池调度）。
//
// RFC 009 §3（2026-08-07）：消除 rt_rw_pool，统一用 work_pool 分配 rt_work_node。
// 原 rt_task_run_work 结构不再单独定义，直接用 rt_work_node 的扩展字段
//（task/action_fn/action_data/ct）。从 3 次 CAS（slab + rt_rw_pool + work_pool）
// 降至 2 次 CAS（slab + work_pool），消除跨线程 rt_rw_pool CAS 竞争。
// trampoline 不再 push node（worker 主循环在 work.fn 执行后自动回收）。

#include "rt_abi.h"
#include <stdlib.h>
#include <stdint.h>
#include <stdatomic.h>
#include <stdio.h> /* atexit */
#include <string.h> /* rt_strdup / memcpy（WriteAllBytesAsync 克隆 byte[]） */

#if defined(_WIN32)
#  define WIN32_LEAN_AND_MEAN
#  define NOMINMAX
#  include <windows.h> /* SEH: EXCEPTION_EXECUTE_HANDLER / EXCEPTION_CONTINUE_SEARCH */
#endif

/* 进程内单例默认池：Task.Run / File.*Async 共用。
 *
 * 退出策略（H1 UnitTest flaky 0xC0000005）：
 * - atexit / WriteReport：Shutdown（join worker）→ Join 未收尾 Thread；
 *   **不** free 池结构；trampoline 的 w/env **漏**至进程退出（禁即时 free(w)，
 *   亦禁报告前集中 free——与 CRT 分配交织仍可损堆）。
 * - Thread opaque drop 须走 destroy→live 表，由 rt_thread_join_live 收尾。 */
static rt_threadpool* g_default_pool = NULL;
static int g_default_pool_atexit = 0;

void rt_default_pool_shutdown(void) {
    if (g_default_pool) {
        rt_threadpool_shutdown(g_default_pool);
        /* 故意不 free 池：见上。 */
        g_default_pool = NULL;
    }
    /* 未 Join 的 Thread.Start：统一 Join，杜绝报告期后台 Action。 */
    rt_thread_join_live();
}

static void rt_default_pool_atexit(void) {
    /* H1: 进程退出期禁止再 join/关池（即使 g_default_pool 非 NULL）。
     * WriteReport 已显式 ShutdownDefaultPool；atexit 与 CRT 析构交织 → AV。 */
    (void)0;
}

static rt_threadpool* rt_default_pool_get(void) {
    if (!g_default_pool) {
        g_default_pool = rt_threadpool_create(0, 0);
        if (!g_default_pool_atexit) {
            atexit(rt_default_pool_atexit);
            g_default_pool_atexit = 1;
        }
    }
    return g_default_pool;
}

/* Task.Run(Action) 工作 trampoline——在 worker 线程上执行 action，完成后标记 Task READY。
 * data 指向 rt_work_node（work.data = node 自身）；trampoline 从 node 扩展字段读取上下文。
 * trampoline 不回收 node——worker 主循环在 work.fn 返回后自动 work_pool_push（RFC 009 §3）。
 *
 * 异常边界（语言缺口修复）：Action 可能抛 Arc 异常（rt_throw 在 Windows 上经
 * `_CxxThrowException` 原生 raise）。纯 C worker 栈无 handler，异常会穿过 trampoline
 * 一路终止进程且 Task 永不置 FAULTED。这里用 Windows SEH 在 C trampoline 边界捕获，
 * 写入 Task 置 FAULTED（rt_task_fault），交由 await/Wait 侧 rethrow——与 codegen 的
 * `rt_task_is_faulted → rt_task_get_exception → rt_throw` 消费端对接。
 * 仅当 TLS `rt_exception` 已置位（确为 Arc 异常）才捕获；外来原生异常（访问违例等）
 * 一律 EXCEPTION_CONTINUE_SEARCH 继续由系统处理，绝不吞噬。
 * POSIX（Itanium EH = milestone ⑨ 落地前，rt_throw → rt_panic 无跨边界异常）走直调。 */
static void rt_task_run_trampoline(void* data) {
    rt_work_node* node = (rt_work_node*)data;
    void (*fn)(void*) = node->action_fn;
    void* d = node->action_data;
    void* task = node->task;

#if defined(_WIN32)
    __try {
        /* 执行用户 Action */
        fn(d);
    } __except (rt_get_exception() != NULL ? EXCEPTION_EXECUTE_HANDLER
                                           : EXCEPTION_CONTINUE_SEARCH) {
        rt_task_fault(task, rt_get_exception());
        return;
    }
#else
    /* 执行用户 Action */
    fn(d);
#endif

    /* 标记 Task 完成 */
    rt_task_set_result_int(task, 0);
    rt_task_complete(task);
    /* node 归还由 worker 主循环处理（work.fn 返回后 work_pool_push） */
}

/* Task.Run<T>(Func<T>) 工作 trampoline——执行返回 int64_t 的 Func，
 * 结果同时存入 int_result 和 ptr_result（兼容值与引用族返回类型）。
 * 异常边界同 rt_task_run_trampoline（Windows SEH 捕获 → rt_task_fault）。 */
static void rt_task_run_func_trampoline(void* data) {
    rt_work_node* node = (rt_work_node*)data;
    int64_t (*fn)(void*) = (int64_t(*)(void*))node->action_fn;
    void* task = node->task;

    /* CT 预检：已取消则直接标记取消，不执行 func */
    if (node->ct && rt_cts_is_canceled(node->ct)) {
        rt_task_cancel(task);
        rt_task_complete(task);
        return;
    }

    /* 执行用户 Func<T>，捕获 int64_t 返回值 */
    int64_t result = 0;
#if defined(_WIN32)
    __try {
        result = fn(node->action_data);
    } __except (rt_get_exception() != NULL ? EXCEPTION_EXECUTE_HANDLER
                                           : EXCEPTION_CONTINUE_SEARCH) {
        rt_task_fault(task, rt_get_exception());
        return;
    }
#else
    result = fn(node->action_data);
#endif

    /* 同时写入两个结果槽：codegen 按 expected 类型取对应槽 */
    rt_task_set_result_int(task, (int32_t)result);
    rt_task_set_result_ptr(task, (void*)(intptr_t)result);

    rt_task_complete(task);
    /* node 归还由 worker 主循环处理 */
}

/* Task.Run 分配的 Task 初始为 READY（slab 默认），须在 spawn 前标为 PENDING，
 * 否则 Wait/await 会在 trampoline 执行前把未写结果的 Task 当作已完成。 */
static void rt_task_run_mark_pending(void* task) {
    if (task) ((RtTask*)task)->status = RT_TASK_PENDING;
}

/* Task.Run(Action) → 在默认线程池上调度，返回 Task*。 */
void* rt_task_run(void* action_fn, void* action_data) {
    void* task = rt_task_slab_alloc();
    if (!task) return NULL;
    rt_task_run_mark_pending(task);

    rt_threadpool* default_pool = rt_default_pool_get();
    rt_work_node* node = rt_work_node_alloc(default_pool);
    if (!node) {
        rt_task_slab_free((RtTask*)task);
        return NULL;
    }
    node->work.fn = rt_task_run_trampoline;
    node->work.data = node;
    node->task = task;
    node->action_fn = (void(*)(void*))action_fn;
    node->action_data = action_data;
    node->ct = NULL;

    rt_threadpool_spawn_node_batched(default_pool, node);

    return task;
}

/* Task.Run(Action, ThreadPoolScheduler) → 在指定线程池上调度，返回 Task*。 */
void* rt_task_run_on_pool(void* pool, void* action_fn, void* action_data) {
    if (!pool) return rt_task_run(action_fn, action_data);

    void* task = rt_task_slab_alloc();
    if (!task) return NULL;
    rt_task_run_mark_pending(task);

    rt_work_node* node = rt_work_node_alloc((rt_threadpool*)pool);
    if (!node) {
        rt_task_slab_free((RtTask*)task);
        return NULL;
    }
    node->work.fn = rt_task_run_trampoline;
    node->work.data = node;
    node->task = task;
    node->action_fn = (void(*)(void*))action_fn;
    node->action_data = action_data;
    node->ct = NULL;

    rt_threadpool_spawn_node_batched((rt_threadpool*)pool, node);

    return task;
}

/* Task.Run<T>(Func<T>) → 在默认线程池上调度，返回 Task<T>*。 */
void* rt_task_run_func(void* func_fn, void* func_data) {
    void* task = rt_task_slab_alloc();
    if (!task) return NULL;
    rt_task_run_mark_pending(task);

    rt_threadpool* default_pool = rt_default_pool_get();
    rt_work_node* node = rt_work_node_alloc(default_pool);
    if (!node) {
        rt_task_slab_free((RtTask*)task);
        return NULL;
    }
    node->work.fn = rt_task_run_func_trampoline;
    node->work.data = node;
    node->task = task;
    node->action_fn = (void(*)(void*))func_fn;
    node->action_data = func_data;
    node->ct = NULL;

    rt_threadpool_spawn_node_batched(default_pool, node);

    return task;
}

/* Task.Run<T>(Func<T>, CancellationToken) → 在默认线程池上调度，带取消令牌。 */
void* rt_task_run_func_ct(void* func_fn, void* func_data, void* ct) {
    void* task = rt_task_slab_alloc();
    if (!task) return NULL;
    rt_task_run_mark_pending(task);

    rt_threadpool* default_pool = rt_default_pool_get();
    rt_work_node* node = rt_work_node_alloc(default_pool);
    if (!node) {
        rt_task_slab_free((RtTask*)task);
        return NULL;
    }
    node->work.fn = rt_task_run_func_trampoline;
    node->work.data = node;
    node->task = task;
    node->action_fn = (void(*)(void*))func_fn;
    node->action_data = func_data;
    node->ct = ct;

    rt_threadpool_spawn_node_batched(default_pool, node);

    return task;
}

/* ---- File.*Async 线程池包装 ---- */
#ifdef _WIN32
  #include <string.h>
  static char* rt_strdup(const char* s) {
      if (!s) return NULL;
      size_t len = strlen(s) + 1;
      char* d = (char*)malloc(len);
      if (d) memcpy(d, s, len);
      return d;
  }
#else
  #define rt_strdup strdup
#endif

static void rt_file_copy_async_tramp(void* raw) {
    struct { void* task; char* src; char* dst; } *w = (void*)raw;
    int32_t result = rt_file_copy(w->src, w->dst);
    rt_task_set_result_int(w->task, result);
    rt_task_complete(w->task);
    /* H1: 勿 free(src/dst/w) */
    (void)w;
}

void* rt_file_copy_async(const char* src, const char* dst) {
    void* task = rt_task_slab_alloc();
    if (!task) return NULL;
    ((RtTask*)task)->status = RT_TASK_PENDING;

    struct { void* task; char* src; char* dst; } *w = malloc(sizeof(*w));
    if (!w) { rt_task_slab_free((RtTask*)task); return NULL; }
    w->task = task;
    w->src = src ? rt_strdup(src) : NULL;
    w->dst = dst ? rt_strdup(dst) : NULL;

    rt_threadpool* pool = rt_default_pool_get();
    rt_work_t work = { rt_file_copy_async_tramp, w };
    rt_threadpool_spawn(pool, work);
    return task;
}

/* --------------- rt_file_move_async --------------- */

static void rt_file_move_async_tramp(void* raw) {
    struct { void* task; char* src; char* dst; } *w = (void*)raw;
    int32_t result = rt_file_move(w->src, w->dst);
    rt_task_set_result_int(w->task, result);
    rt_task_complete(w->task);
    /* H1: 勿 free(src/dst/w) */
    (void)w;
}

void* rt_file_move_async(const char* src, const char* dst) {
    void* task = rt_task_slab_alloc();
    if (!task) return NULL;
    ((RtTask*)task)->status = RT_TASK_PENDING;

    struct { void* task; char* src; char* dst; } *w = malloc(sizeof(*w));
    if (!w) { rt_task_slab_free((RtTask*)task); return NULL; }
    w->task = task;
    w->src = src ? rt_strdup(src) : NULL;
    w->dst = dst ? rt_strdup(dst) : NULL;

    rt_threadpool* pool = rt_default_pool_get();
    rt_work_t work = { rt_file_move_async_tramp, w };
    rt_threadpool_spawn(pool, work);
    return task;
}

/* ------------------------------------------------------------------ */
/* IO Async（RFC 009 异步优先）：元数据 / 目录操作线程池包装。           */
/* ------------------------------------------------------------------ */
/* NOTE：数据面 File.*Async（read_all_text/bytes/lines、write_all_text/
 * bytes、append_all_text）已纠正为 Reactor 真异步（rt_file.c），此处仅保留
 * 无 OS 异步原语的短耗时元数据与目录操作：copy / move / delete / exists /
 * dir_create / dir_exists / dir_delete / dir_list_*。诚实标注：这些是
 * 线程池包装同步 ABI（async-over-sync），非隐藏回退。 */
/* 结果约定：int 族 → rt_task_set_result_int；数组族 → rt_task_set_result_ptr */
/* （trampoline 内新建数组，天然与调用方生命周期解耦）。               */
/* ------------------------------------------------------------------ */

/* rt_file_delete_async：Task<bool> */
static void rt_file_delete_async_tramp(void* raw) {
    struct { void* task; char* path; } *w = (void*)raw;
    int32_t result = rt_file_delete(w->path);
    rt_task_set_result_int(w->task, result);
    rt_task_complete(w->task);
    /* H1: 勿 free(path/w) */
    (void)w;
}

void* rt_file_delete_async(const char* path) {
    void* task = rt_task_slab_alloc();
    if (!task) return NULL;
    ((RtTask*)task)->status = RT_TASK_PENDING;

    struct { void* task; char* path; } *w = malloc(sizeof(*w));
    if (!w) { rt_task_slab_free((RtTask*)task); return NULL; }
    w->task = task;
    w->path = path ? rt_strdup(path) : NULL;

    rt_threadpool* pool = rt_default_pool_get();
    rt_work_t work = { rt_file_delete_async_tramp, w };
    rt_threadpool_spawn(pool, work);
    return task;
}

/* rt_file_exists_async：Task<bool> */
static void rt_file_exists_async_tramp(void* raw) {
    struct { void* task; char* path; } *w = (void*)raw;
    int32_t result = rt_file_exists(w->path);
    rt_task_set_result_int(w->task, result);
    rt_task_complete(w->task);
    /* H1: 勿 free(path/w) */
    (void)w;
}

void* rt_file_exists_async(const char* path) {
    void* task = rt_task_slab_alloc();
    if (!task) return NULL;
    ((RtTask*)task)->status = RT_TASK_PENDING;

    struct { void* task; char* path; } *w = malloc(sizeof(*w));
    if (!w) { rt_task_slab_free((RtTask*)task); return NULL; }
    w->task = task;
    w->path = path ? rt_strdup(path) : NULL;

    rt_threadpool* pool = rt_default_pool_get();
    rt_work_t work = { rt_file_exists_async_tramp, w };
    rt_threadpool_spawn(pool, work);
    return task;
}

/* rt_dir_create_async：Task<bool> */
static void rt_dir_create_async_tramp(void* raw) {
    struct { void* task; char* path; } *w = (void*)raw;
    int32_t result = rt_dir_create(w->path);
    rt_task_set_result_int(w->task, result);
    rt_task_complete(w->task);
    /* H1: 勿 free(path/w) */
    (void)w;
}

void* rt_dir_create_async(const char* path) {
    void* task = rt_task_slab_alloc();
    if (!task) return NULL;
    ((RtTask*)task)->status = RT_TASK_PENDING;

    struct { void* task; char* path; } *w = malloc(sizeof(*w));
    if (!w) { rt_task_slab_free((RtTask*)task); return NULL; }
    w->task = task;
    w->path = path ? rt_strdup(path) : NULL;

    rt_threadpool* pool = rt_default_pool_get();
    rt_work_t work = { rt_dir_create_async_tramp, w };
    rt_threadpool_spawn(pool, work);
    return task;
}

/* rt_dir_exists_async：Task<bool> */
static void rt_dir_exists_async_tramp(void* raw) {
    struct { void* task; char* path; } *w = (void*)raw;
    int32_t result = rt_dir_exists(w->path);
    rt_task_set_result_int(w->task, result);
    rt_task_complete(w->task);
    /* H1: 勿 free(path/w) */
    (void)w;
}

void* rt_dir_exists_async(const char* path) {
    void* task = rt_task_slab_alloc();
    if (!task) return NULL;
    ((RtTask*)task)->status = RT_TASK_PENDING;

    struct { void* task; char* path; } *w = malloc(sizeof(*w));
    if (!w) { rt_task_slab_free((RtTask*)task); return NULL; }
    w->task = task;
    w->path = path ? rt_strdup(path) : NULL;

    rt_threadpool* pool = rt_default_pool_get();
    rt_work_t work = { rt_dir_exists_async_tramp, w };
    rt_threadpool_spawn(pool, work);
    return task;
}

/* rt_dir_delete_async：Task<bool> */
static void rt_dir_delete_async_tramp(void* raw) {
    struct { void* task; char* path; } *w = (void*)raw;
    int32_t result = rt_dir_delete(w->path);
    rt_task_set_result_int(w->task, result);
    rt_task_complete(w->task);
    /* H1: 勿 free(path/w) */
    (void)w;
}

void* rt_dir_delete_async(const char* path) {
    void* task = rt_task_slab_alloc();
    if (!task) return NULL;
    ((RtTask*)task)->status = RT_TASK_PENDING;

    struct { void* task; char* path; } *w = malloc(sizeof(*w));
    if (!w) { rt_task_slab_free((RtTask*)task); return NULL; }
    w->task = task;
    w->path = path ? rt_strdup(path) : NULL;

    rt_threadpool* pool = rt_default_pool_get();
    rt_work_t work = { rt_dir_delete_async_tramp, w };
    rt_threadpool_spawn(pool, work);
    return task;
}

/* rt_dir_list_files_async：Task<string[]> */
static void rt_dir_list_files_async_tramp(void* raw) {
    struct { void* task; char* path; } *w = (void*)raw;
    void* result = rt_dir_list_files(w->path);
    rt_task_set_result_ptr(w->task, result);
    rt_task_complete(w->task);
    /* H1: 勿 free(path/w) */
    (void)w;
}

void* rt_dir_list_files_async(const char* path) {
    void* task = rt_task_slab_alloc();
    if (!task) return NULL;
    ((RtTask*)task)->status = RT_TASK_PENDING;

    struct { void* task; char* path; } *w = malloc(sizeof(*w));
    if (!w) { rt_task_slab_free((RtTask*)task); return NULL; }
    w->task = task;
    w->path = path ? rt_strdup(path) : NULL;

    rt_threadpool* pool = rt_default_pool_get();
    rt_work_t work = { rt_dir_list_files_async_tramp, w };
    rt_threadpool_spawn(pool, work);
    return task;
}

/* rt_dir_list_files_pattern_async：Task<string[]> */
static void rt_dir_list_files_pattern_async_tramp(void* raw) {
    struct { void* task; char* path; char* pattern; } *w = (void*)raw;
    void* result = rt_dir_list_files_pattern(w->path, w->pattern);
    rt_task_set_result_ptr(w->task, result);
    rt_task_complete(w->task);
    /* H1: 勿 free(path/pattern/w) */
    (void)w;
}

void* rt_dir_list_files_pattern_async(const char* path, const char* search_pattern) {
    void* task = rt_task_slab_alloc();
    if (!task) return NULL;
    ((RtTask*)task)->status = RT_TASK_PENDING;

    struct { void* task; char* path; char* pattern; } *w = malloc(sizeof(*w));
    if (!w) { rt_task_slab_free((RtTask*)task); return NULL; }
    w->task = task;
    w->path = path ? rt_strdup(path) : NULL;
    w->pattern = search_pattern ? rt_strdup(search_pattern) : NULL;

    rt_threadpool* pool = rt_default_pool_get();
    rt_work_t work = { rt_dir_list_files_pattern_async_tramp, w };
    rt_threadpool_spawn(pool, work);
    return task;
}

/* rt_dir_list_dirs_async：Task<string[]> */
static void rt_dir_list_dirs_async_tramp(void* raw) {
    struct { void* task; char* path; } *w = (void*)raw;
    void* result = rt_dir_list_dirs(w->path);
    rt_task_set_result_ptr(w->task, result);
    rt_task_complete(w->task);
    /* H1: 勿 free(path/w) */
    (void)w;
}

void* rt_dir_list_dirs_async(const char* path) {
    void* task = rt_task_slab_alloc();
    if (!task) return NULL;
    ((RtTask*)task)->status = RT_TASK_PENDING;

    struct { void* task; char* path; } *w = malloc(sizeof(*w));
    if (!w) { rt_task_slab_free((RtTask*)task); return NULL; }
    w->task = task;
    w->path = path ? rt_strdup(path) : NULL;

    rt_threadpool* pool = rt_default_pool_get();
    rt_work_t work = { rt_dir_list_dirs_async_tramp, w };
    rt_threadpool_spawn(pool, work);
    return task;
}
