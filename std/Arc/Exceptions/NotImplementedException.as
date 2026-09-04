// NotImplementedException — 未实现异常（RFC 027 M0）
// 对标 C# System.NotImplementedException。
namespace Arc;

/// <summary>
/// 方法或操作尚未实现时抛出。通常用于标记 TODO 桩代码。
/// </summary>
public class NotImplementedException : SystemException {
    public NotImplementedException() : base() { }
    public NotImplementedException(string message) : base(message) { }
    public NotImplementedException(string message, Exception? innerException) : base(message, innerException) { }
}
