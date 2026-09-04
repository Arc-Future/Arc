// Task<T> / Task —— async state machine handle (RFC 009)
namespace Arc {

/// <summary>
/// 异步任务句柄，表示返回 T 的异步计算。
/// typeck 将 Task&lt;T&gt; 识别为内置 TypeId::Task { inner }，不起 registry 实例化。
/// 此声明为 stub，方法体不执行；codegen 拦截后直接发射 rt_task_* ABI。
///
/// 非泛型 Task 是 Task&lt;void&gt; 的零成本别名，由 typeck 在 check_type.rs 拦截，
/// 不单独声明 class（Arc 不支持同名泛型/非泛型类）。Task 静态方法
/// (CompletedTask/WhenAll/WhenAny/Run/Delay) 由 typeck check_builtin_static_method
/// 拦截分派，不需要 registry 查找。
/// </summary>
/// <typeparam name="T">结果类型。</typeparam>
public class Task<T> {
    /// <summary>当前任务状态。</summary>
    public TaskStatus Status;
    /// <summary>任务结果（Ready 后可读；否则 panic）。</summary>
    public T Result { get; }
    /// <summary>是否已完成（Ready）。</summary>
    public bool IsCompleted { get; }
    /// <summary>是否已取消。</summary>
    public bool IsCanceled { get; }

    /// <summary>任务失败时封装的异常；未失败时为 null。</summary>
    [Builtin(ABI = "rt_task_get_exception")]
    public Exception? Exception { get; }

    /// <summary>任务是否失败（Status == Faulted）。</summary>
    [Builtin(ABI = "rt_task_is_faulted")]
    public bool IsFaulted { get; }

    /// <summary>读取任务结果（方法形式，与 Result 属性等价）。</summary>
    /// <returns>任务结果。</returns>
    [Builtin(ABI = "rt_task_get_result")]
        public T GetResult() { return default(T); }

    /// <summary>同步阻塞等待任务完成（P1 同步路径为 no-op）。</summary>
    [Builtin(ABI = "rt_task_wait")]
        public void Wait() {}

    /// <summary>标记任务为取消。</summary>
    [Builtin(ABI = "rt_task_cancel")]
        public void Cancel() {}

    /// <summary>创建已完成的 Task&lt;T&gt;（同步路径）。</summary>
    /// <param name="value">任务结果值。</param>
    /// <returns>已完成的 Task&lt;T&gt;。</returns>
    [Builtin(ABI = "rt_task_from_result")]
        public static Task<T> FromResult(T value) { return null; }

    /// <summary>已完成的 void Task（同步路径，Task&lt;void&gt; 别名）。</summary>
    [Builtin(ABI = "rt_task_completed")]
        public static Task CompletedTask { get; }

    /// <summary>等待全部 Task 完成（可变实参走 <c>params ReadOnlySpan&lt;Task&gt;</c> 栈脱糖，零堆；RFC 005 M2a）。</summary>
    /// <param name="tasks">待等待的 Task（<c>WhenAll(t1, t2, …)</c> / 空参 / 既有 ROS 直传）。</param>
    [Builtin(ABI = "rt_task_when_all")]
    public static Task WhenAll(params ReadOnlySpan<Task> tasks) { return null; }

    /// <summary>等待任一 Task 完成（可变实参走 <c>params ReadOnlySpan&lt;Task&gt;</c> 栈脱糖，零堆；RFC 005 M2a）。</summary>
    /// <param name="tasks">待等待的 Task（<c>WhenAny(t1, t2, …)</c> / 空参 / 既有 ROS 直传）。</param>
    [Builtin(ABI = "rt_task_when_any")]
    public static Task WhenAny(params ReadOnlySpan<Task> tasks) { return null; }

    /// <summary>在默认线程池上调度 Action 并返回 Task（RFC 009 M5.7；L2 最小可宣称）。</summary>
    /// <param name="action">要执行的任务委托。</param>
    /// <returns>表示该后台工作的 Task。</returns>
    [Builtin(ABI = "rt_task_run")]
    public static Task Run(Action action) { return null; }

    /// <summary>在指定线程池上调度 Action 并返回 Task（显式 ThreadPoolScheduler；L2 基本 API Stable）。</summary>
    /// <param name="action">要执行的任务委托。</param>
    /// <param name="scheduler">目标调度器（ThreadPoolScheduler）。</param>
    /// <returns>表示该后台工作的 Task。</returns>
    [Builtin(ABI = "rt_task_run_on_pool")]
    public static Task Run(Action action, Threading.ThreadPoolScheduler scheduler) { return null; }

    /// <summary>创建在指定毫秒后完成的 Task（定时器异步等待）。</summary>
    /// <param name="milliseconds">延迟毫秒数。</param>
    /// <returns>延迟完成后变为 Ready 的 Task。</returns>
    [Builtin(ABI = "rt_task_delay")]
    public static Task Delay(int milliseconds) { return null; }

    /// <summary>创建可取消的延迟 Task。</summary>
    /// <param name="milliseconds">延迟毫秒数。</param>
    /// <param name="cancellationToken">取消令牌；取消时 Task 变为 Canceled。</param>
    /// <returns>延迟完成后变为 Ready（或被取消变为 Canceled）的 Task。</returns>
    [Builtin(ABI = "rt_task_delay")]
    public static Task Delay(int milliseconds, CancellationToken cancellationToken) { return null; }

    /// <summary>创建已取消的 Task（无结果）。</summary>
    /// <param name="cancellationToken">取消令牌（用于传递取消原因）。</param>
    /// <returns>状态为 Canceled 的 Task。</returns>
    [Builtin(ABI = "rt_task_from_canceled")]
    public static Task FromCanceled(CancellationToken cancellationToken) { return null; }

    /// <summary>创建已取消的 Task&lt;T&gt;（带结果类型）。</summary>
    /// <typeparam name="T">结果类型。</typeparam>
    /// <param name="cancellationToken">取消令牌（用于传递取消原因）。</param>
    /// <returns>状态为 Canceled 的 Task&lt;T&gt;。</returns>
    [Builtin(ABI = "rt_task_from_canceled")]
    public static Task<T> FromCanceled<T>(CancellationToken cancellationToken) { return null; }

    /// <summary>同步阻塞等待全部 Task 完成（<c>params ReadOnlySpan&lt;Task&gt;</c>）。</summary>
    /// <param name="tasks">待等待的 Task（<c>WaitAll(t1, t2, …)</c> / 空参 / ROS 直传）。</param>
    [Builtin(ABI = "rt_task_wait_all")]
    public static void WaitAll(params ReadOnlySpan<Task> tasks) {}

    /// <summary>同步阻塞等待任一 Task 完成，返回已完成 Task 的索引（<c>params ReadOnlySpan&lt;Task&gt;</c>）。</summary>
    /// <param name="tasks">待等待的 Task（<c>WaitAny(t1, t2, …)</c> / 空参 / ROS 直传）。</param>
    /// <returns>第一个完成的 Task 在实参中的索引。</returns>
    [Builtin(ABI = "rt_task_wait_any")]
    public static int WaitAny(params ReadOnlySpan<Task> tasks) { return -1; }

    /// <summary>同步阻塞等待任务完成，最长等待 timeoutMs 毫秒。</summary>
    /// <param name="timeoutMs">超时毫秒数（0 表示无限等待）。</param>
    /// <returns>true 在超时前完成；false 超时。</returns>
    [Builtin(ABI = "rt_task_wait")]
    public bool Wait(int timeoutMs) { return false; }

    /// <summary>同步阻塞等待任务完成，支持取消令牌中断等待。</summary>
    /// <param name="cancellationToken">取消令牌。</param>
    /// <returns>true 正常完成；false 被取消中断。</returns>
    [Builtin(ABI = "rt_task_wait")]
    public bool Wait(CancellationToken cancellationToken) { return false; }

    /// <summary>调度 Func&lt;T&gt; 在默认线程池执行并返回 Task&lt;T&gt;。</summary>
    /// <typeparam name="T">结果类型。</typeparam>
    /// <param name="function">返回 T 的函数委托。</param>
    /// <returns>表示该后台工作的 Task&lt;T&gt;。</returns>
    [Builtin(ABI = "rt_task_run")]
    public static Task<T> Run<T>(Func<T> function) { return null; }

    /// <summary>调度 Func&lt;T&gt; 在默认线程池执行（带取消令牌）并返回 Task&lt;T&gt;。</summary>
    /// <typeparam name="T">结果类型。</typeparam>
    /// <param name="function">返回 T 的函数委托。</param>
    /// <param name="cancellationToken">取消令牌。</param>
    /// <returns>表示该后台工作的 Task&lt;T&gt;。</returns>
    [Builtin(ABI = "rt_task_run")]
    public static Task<T> Run<T>(Func<T> function, CancellationToken cancellationToken) { return null; }

    /// <summary>等待全部 Task&lt;T&gt; 完成，返回各 Task 结果数组。</summary>
    /// <typeparam name="T">任务结果类型。</typeparam>
    /// <param name="tasks">待等待的 Task&lt;T&gt; 数组。</param>
    /// <returns>包含所有 Task 结果的 Task&lt;T[]&gt;。</returns>
    [Builtin(ABI = "rt_task_when_all")]
    public static Task<T[]> WhenAll<T>(Task<T>[] tasks) { return null; }

    /// <summary>等待任一 Task&lt;T&gt; 完成，返回该 Task。</summary>
    /// <typeparam name="T">任务结果类型。</typeparam>
    /// <param name="tasks">待等待的 Task&lt;T&gt; 数组。</param>
    /// <returns>表示第一个完成的 Task 的 Task&lt;Task&lt;T&gt;&gt;。</returns>
    [Builtin(ABI = "rt_task_when_any")]
    public static Task<Task<T>> WhenAny<T>(Task<T>[] tasks) { return null; }

    /// <summary>配置 await 行为：是否在捕获的上下文上恢复执行。</summary>
    /// <param name="continueOnCapturedContext">true 在捕获上下文恢复；false 在线程池恢复。</param>
    /// <returns>await-ready 的 Task（Arc 无 SynchronizationContext，此方法为伪等映射）。</returns>
    [Builtin(ABI = "rt_task_configure_await")]
    public Task<T> ConfigureAwait(bool continueOnCapturedContext) { return this; }

    // ── 异常工厂 ──

    /// <summary>创建已失败的 Task（Status = Faulted；Exception 可回读）。</summary>
    [Builtin(ABI = "rt_task_from_exception")]
    public static Task FromException(Exception exception) { return null; }

    /// <summary>创建已失败的 Task&lt;T&gt;（Status = Faulted；Exception 可回读）。</summary>
    [Builtin(ABI = "rt_task_from_exception")]
    public static Task<T> FromException<T>(Exception exception) { return null; }

    // Task.Yield ABI / 调度器让步后置——禁止 null stub 假面（已撤面）。
}

} // namespace Arc
