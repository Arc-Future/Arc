// ParallelResult —— 并行循环结果（RFC 009 §5.3 / RFC 009 M5.7）。
namespace Arc.Threading {

/// <summary>
/// 并行循环结果（C# TPL 对齐）。由 Parallel.For / Parallel.ForEach 返回，
/// 携带本次循环实际完成的迭代数。
///
/// Arc struct 仅支持 field（无 property/方法），与 RFC 009 §5.3 中
/// `struct ParallelResult { public int CompletedCount; }` 定义一致。
/// 由 codegen 在 rt_parallel_for / rt_parallel_foreach 调用后构造：
/// 将 ABI 返回的分区数填入 CompletedCount 字段。
/// </summary>
public struct ParallelResult {
    /// <summary>本次循环已完成的迭代分区数。</summary>
    public int CompletedCount;

    /// <summary>循环是否正常完成（未被 Break/Stop 提前终止）。</summary>
    public bool IsCompleted;
}

} // namespace Arc.Threading
