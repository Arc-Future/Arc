// NotSupportedException — 不支持异常（RFC 027 M0）
// 对标 C# System.NotSupportedException。
namespace Arc;

/// <summary>
/// 方法不被支持时抛出。与 NotImplementedException 的区别：
/// 此异常表示方法有意不支持（而非暂未实现）。
/// </summary>
public class NotSupportedException : SystemException {
    public NotSupportedException() : base() { }
    public NotSupportedException(string message) : base(message) { }
    public NotSupportedException(string message, Exception? innerException) : base(message, innerException) { }
}
