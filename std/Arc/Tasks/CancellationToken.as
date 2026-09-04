// CancellationToken — 协作式取消的只读句柄（RFC 009 §6.1 / M4）。
namespace Arc {

/// <summary>
/// 取消令牌——协作式取消的只读句柄。typeck 拦截为 TypeId::Named("CancellationToken")；
/// codegen 拦截后发射 rt_cts_* ABI。CT 与 CTS 共享同一 RtCts* 指针（CT 是只读视图，
/// D2 决策：避免双层间接，MVP 单线程足够）。
///
/// 此声明为 stub，方法体不执行；codegen 在 emit_call.rs 的 try_emit_ct_method
/// 中直接发射 @rt_cts_* ABI。
///
/// ThrowIfCancellationRequested 反糖为 C# 等价语义
/// `if (IsCancellationRequested) throw new OperationCanceledException()`——
/// 异常经统一异常通道抛出，async 状态机 / await 方 catch 可捕获。
/// </summary>
public class CancellationToken {
    /// <summary>是否已请求取消。</summary>
    [Builtin(ABI = "rt_ct_is_cancellation_requested")]
    public bool IsCancellationRequested { get; }

    /// <summary>是否永不可取消（None 令牌恒为 false）。</summary>
    [Builtin(ABI = "rt_ct_can_be_canceled")]
    public bool CanBeCanceled { get; }

    /// <summary>已取消则抛 OperationCanceledException（经统一异常通道，可被 await 方 catch）。</summary>
    public void ThrowIfCancellationRequested() {}

    /// <summary>注册取消时回调。</summary>
    /// <param name="callback">取消时调用的回调（Action 闭包）。</param>
    [Builtin(ABI = "rt_ct_register")]
    public void Register(Action callback) {}

    /// <summary>永不可取消的空令牌。所有接受 CancellationToken 的 API 默认参数值。</summary>
    [Builtin(ABI = "rt_ct_none")]
    public static CancellationToken None { get; }
}

} // namespace Arc
