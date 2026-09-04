// CancellationTokenSource — 协作式取消的控制端（RFC 009 §6.1 / M4）。
namespace Arc {

/// <summary>
/// 取消令牌源——取消的控制端。typeck 拦截为 TypeId::Named("CancellationTokenSource")；
/// codegen 拦截后发射 rt_cts_* ABI。CT 与 CTS 共享同一 RtCts* 指针（CT 是只读视图）。
///
/// 此声明为 stub，方法体不执行；codegen 在 emit_call.rs 的 try_emit_cts_method
/// 中直接发射 @rt_cts_* ABI。new() 在 emit_new.rs 拦截为 @rt_cts_create()。
/// </summary>
public class CancellationTokenSource {
    /// <summary>关联的取消令牌（只读视图，与 CTS 共享指针）。</summary>
    [Builtin(ABI = "rt_cts_token")]
    public CancellationToken Token { get; }

    /// <summary>是否已请求取消。</summary>
    [Builtin(ABI = "rt_cts_is_cancellation_requested")]
    public bool IsCancellationRequested { get; }

    /// <summary>触发取消。</summary>
    [Builtin(ABI = "rt_cts_cancel")]
    public void Cancel() {}

    /// <summary>延迟触发取消。</summary>
    [Builtin(ABI = "rt_cts_cancel_after")]
    public void CancelAfter(int milliseconds) {}

    /// <summary>释放 CTS 持有的非托管资源（定时器等）。</summary>
    [Builtin(ABI = "rt_cts_destroy")]
    public void Dispose() {}

    // ── 静态 ──

    /// <summary>创建链接取消令牌源——任意一个令牌取消则触发联动取消。</summary>
    /// <param name="tokens">要链接的取消令牌数组。</param>
    /// <returns>新 CTS，其中任一 token 取消时自动触发。</returns>
    [Builtin(ABI = "rt_cts_create_linked")]
    public static CancellationTokenSource CreateLinkedTokenSource(CancellationToken[] tokens) { return null; }

    /// <summary>创建链接取消令牌源——token1 或 token2 任一取消时触发。</summary>
    [Builtin(ABI = "rt_cts_create_linked_pair")]
    public static CancellationTokenSource CreateLinkedTokenSource(CancellationToken token1, CancellationToken token2) { return null; }
}

} // namespace Arc
