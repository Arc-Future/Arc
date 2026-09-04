// AIModelCancelledException — 协作式取消（RFC 041 §7.3）。
//
// 调用方 CancellationToken 取消时，服务骨架把 OperationCanceledException 收敛为
// 本类型（统一小模型错误层次；不裸透运行时取消异常）。非重试（取消不重试）。
namespace Arc.AI;

using Arc;

/// <summary>模型调用被协作式取消（RFC 041 §7.3）。</summary>
public class AIModelCancelledException : AIModelException {
    public AIModelCancelledException() : base() { }
    public AIModelCancelledException(string message) : base(message) { }
    public AIModelCancelledException(string message, Exception? innerException) : base(message, innerException) { }
}
