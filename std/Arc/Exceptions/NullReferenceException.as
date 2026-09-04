// NullReferenceException — 空引用异常（RFC 027 M0）
// 对标 C# System.NullReferenceException。
namespace Arc;

/// <summary>
/// 对 null 引用执行成员访问时抛出。
/// </summary>
public class NullReferenceException : SystemException {
    public NullReferenceException() : base() { }
    public NullReferenceException(string message) : base(message) { }
    public NullReferenceException(string message, Exception? innerException) : base(message, innerException) { }
}
