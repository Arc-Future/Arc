// FormatException — 格式异常（RFC 027 M0）
// 对标 C# System.FormatException。
namespace Arc;

/// <summary>
/// 参数格式不符合要求时抛出。
///
/// 示例：
///   - int.Parse("abc")
///   - Guid.Parse("not-a-guid")
/// </summary>
public class FormatException : SystemException {
    public FormatException() : base() { }
    public FormatException(string message) : base(message) { }
    public FormatException(string message, Exception? innerException) : base(message, innerException) { }
}
