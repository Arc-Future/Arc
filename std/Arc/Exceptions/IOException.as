// IOException — I/O 异常（RFC 027 M0）
// 对标 C# System.IO.IOException。
namespace Arc;

/// <summary>
/// I/O 错误时抛出。
/// </summary>
public class IOException : SystemException {
    public IOException(string message) : base(message) { }
    public IOException(string message, Exception? innerException) : base(message, innerException) { }
}
