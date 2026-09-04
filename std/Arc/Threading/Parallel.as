// Parallel — 并行循环原语（RFC 009 §5.3 / RFC 009 M5.7）
// [Builtin] 标记的方法为 codegen stub，body 不执行；无 [Builtin] 的方法为真实 Arc 代码。
// 对齐 C# System.Threading.Tasks.Parallel（ForEachAsync 直接在 Parallel 类上）。
namespace Arc.Threading {

/// <summary>
/// 并行循环原语（C# TPL 对齐）。For/ForEach 将工作区间分区后
/// 分发到 ThreadPoolScheduler 并行执行。
///
/// API 表面对齐 RFC 009 §5.3：
///   - For(fromInclusive, toExclusive, body) / For(fromInclusive, toExclusive, options, body)
///   - ForEach&lt;T&gt;(source, body) / ForEach&lt;T&gt;(source, options, body)
///   - ForAsync / ForEachAsync 异步版本（.NET 6+ 风格）
///   - 返回 ParallelResult（携带 CompletedCount）
///
/// [Builtin] 方法为 stub，body 不执行；codegen 拦截后直接发射 @rt_parallel_for /
/// @rt_parallel_foreach ABI。无 [Builtin] 的 async 方法为真实 Arc 代码。
/// </summary>
public class Parallel {
    /// <summary>并行 for 循环（默认线程池，不限制并发度，不启用取消）。</summary>
    [Builtin(ABI = "rt_parallel_for")]
    public static ParallelResult For(int fromInclusive, int toExclusive, Action<int> body) { return default(ParallelResult); }

    /// <summary>并行 for 循环（带 ParallelOptions：调度器 / 并发度限制 / 取消令牌）。</summary>
    [Builtin(ABI = "rt_parallel_for")]
    public static ParallelResult For(int fromInclusive, int toExclusive, ParallelOptions options, Action<int> body) { return default(ParallelResult); }

    /// <summary>并行 foreach 循环（默认线程池，不限制并发度，不启用取消）。</summary>
    [Builtin(ABI = "rt_parallel_foreach")]
    public static ParallelResult ForEach<T>(IEnumerable<T> source, Action<T> body) { return default(ParallelResult); }

    /// <summary>并行 foreach 循环（带 ParallelOptions：调度器 / 并发度限制 / 取消令牌）。</summary>
    [Builtin(ABI = "rt_parallel_foreach")]
    public static ParallelResult ForEach<T>(IEnumerable<T> source, ParallelOptions options, Action<T> body) { return default(ParallelResult); }

    // ---- Async 版本（M5.7）：纯 Arc 代码，基于 Task.Run + Parallel ----

    /// <summary>异步并行 for 循环（默认线程池）。将并行循环提交到线程池，立即返回 Task 而不阻塞调用线程。</summary>
    public static Task<ParallelResult> ForAsync(int fromInclusive, int toExclusive, Action<int> body) {
        return Task.Run<ParallelResult>(() => Parallel.For(fromInclusive, toExclusive, body));
    }

    /// <summary>异步并行 for 循环（带 ParallelOptions）。</summary>
    public static Task<ParallelResult> ForAsync(int fromInclusive, int toExclusive, ParallelOptions options, Action<int> body) {
        return Task.Run<ParallelResult>(() => Parallel.For(fromInclusive, toExclusive, options, body));
    }

    /// <summary>异步并行 foreach 循环（默认线程池）。</summary>
    public static Task<ParallelResult> ForEachAsync<T>(IEnumerable<T> source, Action<T> body) {
        return Task.Run<ParallelResult>(() => Parallel.ForEach<T>(source, body));
    }

    /// <summary>异步并行 foreach 循环（带 ParallelOptions）。</summary>
    public static Task<ParallelResult> ForEachAsync<T>(IEnumerable<T> source, ParallelOptions options, Action<T> body) {
        return Task.Run<ParallelResult>(() => Parallel.ForEach<T>(source, options, body));
    }

    // ── Invoke ──

    /// <summary>并行执行多个独立 Action。所有 Action 完成后返回。</summary>
    [Builtin(ABI = "rt_parallel_invoke")]
    public static void Invoke(Action action) {}

    /// <summary>并行执行多个独立 Action（带 ParallelOptions）。</summary>
    [Builtin(ABI = "rt_parallel_invoke")]
    public static void Invoke(ParallelOptions options, Action action) {}
}

} // namespace Arc.Threading
