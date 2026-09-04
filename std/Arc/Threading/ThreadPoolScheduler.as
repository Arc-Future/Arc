// ThreadPoolScheduler — CPU 密集任务并行调度器（RFC 009 §5.2 / M5.1 / RFC 009 §5.2）
namespace Arc.Threading {

/// <summary>
/// CPU 密集任务并行调度器。基于 Chase-Lev work-stealing deque，
/// N worker 线程池 + 全局 MPSC injector queue + TLS worker_id。
///
/// 默认参数（RFC 007 M3）：workerCount=0 自动取 CPU 核数；numaAware=false 默认关闭（跨平台兼容）。
/// n_workers&lt;=0 → hardware_concurrency()；numaAware=true 启用 worker→NUMA node 绑定（单 node / 不支持平台为 no-op）。
///
/// L2 Stable：ctor（含可选/命名实参）/ Run / PendingTaskCount / ActiveWorkerCount / Shutdown / Destroy；
/// 多任务压力完成面；协作式抢占检查路径（await 边界）。方法体为 facade stub。
/// </summary>
public class ThreadPoolScheduler {
    /// <summary>创建 CPU 密集线程池。</summary>
    /// <param name="workerCount">worker 数（0 = CPU 核数，即 hardware_concurrency()）。</param>
    /// <param name="numaAware">是否启用 NUMA 感知调度（多 node 时绑定；单 node 为 no-op）。</param>
    /// <remarks>typeck 按 RFC 007 脱糖填缺省；codegen 拦截 <c>new ThreadPoolScheduler(...)</c>
    /// → <c>rt_threadpool_create</c>（满 arity 实参）。</remarks>
    [Builtin(ABI = "rt_threadpool_create")]
    public ThreadPoolScheduler(int workerCount = 0, bool numaAware = false) {}

    /// <summary>在池上调度任务并返回 Task。</summary>
    /// <param name="action">要执行的任务委托。</param>
    /// <returns>表示该后台工作的 Task。</returns>
    [Builtin(ABI = "rt_threadpool_run")]
    public Task Run(Action action) { return null; }

    /// <summary>在池上调度任务并返回 Task&lt;T&gt;。</summary>
    /// <typeparam name="T">结果类型。</typeparam>
    /// <param name="func">返回 T 的任务委托。</param>
    /// <returns>表示该后台工作的 Task&lt;T&gt;。</returns>
    [Builtin(ABI = "rt_threadpool_run_task")]
    public Task<T> Run<T>(Func<T> func) { return null; }

    /// <summary>池配置的 worker 数（create 时确定；非瞬时 busy 计数）。</summary>
    [Builtin(ABI = "rt_threadpool_worker_count")]
    public int ActiveWorkerCount { get; }

    /// <summary>待处理任务数（排队未开始执行；任务开始执行即递减，故已完成时恒为 0）。</summary>
    [Builtin(ABI = "rt_threadpool_pending_task_count")]
    public int PendingTaskCount { get; }

    /// <summary>关闭线程池：等待空闲并 join worker。不 free 池结构；完整回收见 <see cref="Destroy"/>。</summary>
    [Builtin(ABI = "rt_threadpool_shutdown")]
    public void Shutdown() {}

    /// <summary>
    /// 安全销毁：wait_idle + join（可接在 Shutdown 后）。H1：Join 后不 free 池/deque
    ///（与默认池同策——报告期 CRT 分配窗口禁中途 free）；结构漏至进程退出。
    /// 禁止销毁后继续 spawn。
    /// </summary>
    [Builtin(ABI = "rt_threadpool_destroy")]
    public void Destroy() {}

    /// <summary>
    /// Join 进程默认 Task.Run 池（不 free）。报告/退出前调用，避免 WriteResults 与 worker 堆竞态。
    /// atexit 仍会兜底 join；本入口缩短窗口。
    /// </summary>
    [Builtin(ABI = "rt_default_pool_shutdown")]
    public static void ShutdownDefaultPool() {}
}

} // namespace Arc.Threading
