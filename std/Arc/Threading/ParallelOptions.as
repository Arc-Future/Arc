// ParallelOptions —— 并行循环选项（RFC 009 §5.3 / RFC 009 M5.7）。
namespace Arc.Threading {

/// <summary>
/// 并行循环选项（C# TPL 对齐）。作为 Parallel.For / Parallel.ForEach 的
/// 可选参数，控制调度器、最大并发度与协作式取消。
///
/// 字段对齐 RFC 009 §5.3：
///   - Scheduler：调度器（RFC 中类型为 TaskScheduler，Arc 未引入 TaskScheduler
///     抽象，直接复用 ThreadPoolScheduler 具体类型——零开销抽象 + 显式优于隐式）。
///   - MaxDegreeOfParallelism：默认 -1（不限），rt_parallel_for 接收 max_degree
///     参数时 ≤0 表示不限制。
///   - CancellationToken：默认 None（CancellationToken 在 Arc 根命名空间，
///     typeck 通过 is_builtin_facade 名字解析，无需 using）。
///
/// 此声明为 stub；属性体不执行。codegen emit_parallel_for 通过 GEP 按字段顺序
/// 提取 Scheduler(ptr)/MaxDegreeOfParallelism(i32)/CancellationToken(ptr) 三个 slot，
/// 转换为 rt_parallel_for 的 pool/cts/max_degree 实参。
/// </summary>
public class ParallelOptions {
    /// <summary>调度器；null 表示使用默认线程池（rt_parallel_for pool=null 路径）。</summary>
    public ThreadPoolScheduler Scheduler { get; set; }

    /// <summary>最大并发度；-1 表示不限（rt_parallel_for max_degree≤0 路径）。</summary>
    public int MaxDegreeOfParallelism { get; set; }

    /// <summary>取消令牌；默认 None（rt_parallel_for cts=null 路径）。</summary>
    public CancellationToken CancellationToken { get; set; }
}

} // namespace Arc.Threading
