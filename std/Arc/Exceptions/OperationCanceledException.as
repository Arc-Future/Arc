// OperationCanceledException — 操作已取消异常（RFC 027 M0 / RFC 009 M4）
// 对标 C# System.OperationCanceledException。
namespace Arc;

/// <summary>
/// 操作被取消时抛出，通常与 CancellationToken 协作。
/// </summary>
public class OperationCanceledException : SystemException {
    public OperationCanceledException() : base() { }
    public OperationCanceledException(string message) : base(message) { }
    public OperationCanceledException(string message, Exception? innerException) : base(message, innerException) { }
}
