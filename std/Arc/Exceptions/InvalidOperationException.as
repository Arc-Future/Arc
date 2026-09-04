// InvalidOperationException — 无效操作异常（RFC 027 M0）
// 对标 C# System.InvalidOperationException。
namespace Arc;

/// <summary>
/// 方法调用对于对象当前状态无效时抛出。
///
/// 示例：
///   - 对空集合调用 Pop()
///   - 在枚举过程中修改集合
/// </summary>
public class InvalidOperationException : SystemException {
    public InvalidOperationException() : base() { }
    public InvalidOperationException(string message) : base(message) { }
    public InvalidOperationException(string message, Exception? innerException) : base(message, innerException) { }
}
