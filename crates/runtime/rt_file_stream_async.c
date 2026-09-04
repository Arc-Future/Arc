//! FileStream 真异步 runtime ABI（文件 I/O 线程池卸载 + EventLoop 完成投递）。
//!
//! 符号前缀：`rt_file_stream_*_async`。对标 .NET FileStream（isAsync: false 路径）
//! 的异步语义层：阻塞 I/O 卸载到专用文件 I/O 工作线程池，完成后向事件循环投递
//! 完成信号唤醒 await——语义上对标 IOCP/overlapped 完成端口线程池。
//!
//! 为什么不是 Reactor overlapped（File.*Async 的 rt_file.c 路线）：
//!   - FileStream 同步面（Stable，RFC 036 冻结）持有 CRT FILE*（内部缓冲 + 文件
//!     位置状态）。Reactor 路线要求 FILE_FLAG_OVERLAPPED/io_uring 专用句柄，会与
//!     FILE* 形成双句柄双位置，破坏 Stream 单一 Position 语义；替换同步面句柄
//!     模型属破坏性变更（冻结面禁止）。
//!   - Windows 文件句柄无 readiness（epoll/select）语义，socket Reactor 模型
//!     不适用于文件；行业实践（.NET FileStream 默认路径）即线程池卸载。
//!
//! 完成信号路径（与 Task.Run 同构，已验证的跨线程唤醒链路）：
//!   worker 线程执行 fread/fwrite/fflush → rt_task_set_result_* + rt_task_complete
//!   → Task READY + 触发 waker → g_rt_wake_fn（rt_task_default_wake）
//!   → rt_event_loop_spawn（mutex + 就绪队列 + condvar signal）→ EventLoop 唤醒
//!   → await 状态机恢复。调用线程提交后立即返回 Pending Task（真异步，非
//!   sync-over-async：阻塞发生在池线程，调用线程零等待）。
//!
//! 隔离设计：文件 I/O 专用池实例独立于 Task.Run 默认池（rt_task_run.c）——阻塞
//! 文件操作不侵占 CPU 密集 worker（对标 .NET 完成端口线程与 worker 线程分池语义）。
//! 池内核复用 rt_threadpool.c（work-stealing），worker 数 min(4, hardware)：
//! 文件 I/O 为阻塞等待型负载，小规模池即可支撑典型并发（.NET min IOCP 线程=1
//! 起步按需增长；此处固定小池，简单且可预期）。
//!
//! 取消语义（诚实边界，对齐 rt_task_run_func_trampoline 先例）：仅提交前预检
//! CT——已取消则返回已取消 Task，不占池线程；操作一旦进入池线程执行则不可中止
//! （CRT fread 无取消原语；.NET FileStream 非重叠路径同样无法中止已开始的 OS 读）。
//!
//! 生命周期：工作项 malloc 后**不 free**（H1 退出策略，与 rt_task_run.c 元数据
//! 包装一致——退出期与 CRT 析构交织的 free 可损堆；量级为每异步调用一次，进程
//! 生命周期内可忽略）。池退出走显式 rt_file_stream_async_shutdown（Shutdown +
//! join，不 free 池结构）；async main wrapper 不自动调用（与默认池策略一致，
//! ExitProcess 终止 worker；await 语义下 root task 完成即全部被等待操作已完成）。

#include "rt_abi.h"

#include <stdlib.h>
#include <stddef.h>

#ifdef _WIN32
  #include <windows.h>
#else
  #include <unistd.h> /* sysconf(_SC_NPROCESSORS_ONLN) */
#endif

/* ---- 文件 I/O 专用线程池（惰性创建，进程内单例）---- */

static rt_threadpool* g_file_io_pool = NULL;

static int32_t rt_file_io_pool_worker_count(void) {
#ifdef _WIN32
    SYSTEM_INFO si;
    GetSystemInfo(&si);
    int32_t n = (int32_t)si.dwNumberOfProcessors;
#else
    long n = sysconf(_SC_NPROCESSORS_ONLN);
    if (n <= 0) n = 1;
#endif
    return n > 4 ? 4 : (n < 1 ? 1 : n);
}

static rt_threadpool* rt_file_io_pool_get(void) {
    if (!g_file_io_pool) {
        g_file_io_pool = rt_threadpool_create(rt_file_io_pool_worker_count(), 0);
    }
    return g_file_io_pool;
}

void rt_file_stream_async_shutdown(void) {
    if (g_file_io_pool) {
        rt_threadpool_shutdown(g_file_io_pool);
        /* 故意不 free 池：退出期 free 与 CRT 析构交织可致 AV（H1，同默认池）。 */
        g_file_io_pool = NULL;
    }
}

/* ---- 异步工作项 ---- */

typedef enum {
    RT_FS_ASYNC_READ  = 0,
    RT_FS_ASYNC_WRITE = 1,
    RT_FS_ASYNC_FLUSH = 2,
} RtFsAsyncOp;

typedef struct RtFileStreamWork {
    void*    task;    /* 关联 Pending Task */
    void*    handle;  /* RtFileStream*（rt_file_stream.c 不透明句柄） */
    void*    buffer;  /* byte[] payload（调用方所有，Task 完成前保持有效） */
    int32_t  offset;
    int32_t  count;
    int32_t  op;      /* RtFsAsyncOp */
} RtFileStreamWork;

/* 池线程 trampoline：执行阻塞 I/O → 写回结果 → 完成投递（唤醒 EventLoop）。
 * 复用同步面 ABI（rt_file_stream_read/write/flush）：FILE* 位置状态由 CRT
 * 单点管理，同步/异步混用时同一 Position 语义不被分裂（CRT per-handle 锁
 * 保证并发安全）。 */
static void rt_file_stream_async_tramp(void* raw) {
    RtFileStreamWork* w = (RtFileStreamWork*)raw;
    int32_t result = 0;
    if (w->op == RT_FS_ASYNC_READ) {
        result = rt_file_stream_read(w->handle, w->buffer, w->offset, w->count);
    } else if (w->op == RT_FS_ASYNC_WRITE) {
        rt_file_stream_write(w->handle, w->buffer, w->offset, w->count);
        result = 1;
    } else { /* RT_FS_ASYNC_FLUSH */
        rt_file_stream_flush(w->handle);
        result = 1;
    }
    rt_task_set_result_int(w->task, result);
    rt_task_complete(w->task);
    /* H1: 勿 free(w)——见文件头生命周期说明。 */
}

/* 共享提交：CT 预检 → Pending Task → 工作项 → 池 spawn。失败返回 NULL。 */
static void* rt_file_stream_submit(void* handle, void* buffer, int32_t offset,
                                   int32_t count, void* ct, int32_t op) {
    if (!handle) return NULL;
    if (ct && rt_cts_is_canceled(ct)) {
        /* 提交前已取消：返回已取消 Task，不占池线程（先例 rt_task_run_func_trampoline）。 */
        RtTask* task = rt_task_slab_alloc();
        if (!task) return NULL;
        task->status = RT_TASK_PENDING;
        rt_task_cancel(task);
        rt_task_complete(task);
        return task;
    }

    rt_threadpool* pool = rt_file_io_pool_get();
    if (!pool) return NULL;

    RtTask* task = rt_task_slab_alloc();
    if (!task) return NULL;
    task->status = RT_TASK_PENDING;

    RtFileStreamWork* w = (RtFileStreamWork*)malloc(sizeof(RtFileStreamWork));
    if (!w) {
        rt_task_slab_free(task);
        return NULL;
    }
    w->task = task;
    w->handle = handle;
    w->buffer = buffer;
    w->offset = offset;
    w->count = count;
    w->op = op;

    rt_work_t work = { rt_file_stream_async_tramp, w };
    rt_threadpool_spawn(pool, work);
    return task;
}

void* rt_file_stream_read_async(void* handle, void* buffer, int32_t offset,
                                int32_t count, void* ct) {
    return rt_file_stream_submit(handle, buffer, offset, count, ct, RT_FS_ASYNC_READ);
}

void* rt_file_stream_write_async(void* handle, void* buffer, int32_t offset,
                                 int32_t count, void* ct) {
    return rt_file_stream_submit(handle, buffer, offset, count, ct, RT_FS_ASYNC_WRITE);
}

void* rt_file_stream_flush_async(void* handle, void* ct) {
    return rt_file_stream_submit(handle, NULL, 0, 0, ct, RT_FS_ASYNC_FLUSH);
}
