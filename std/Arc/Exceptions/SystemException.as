// SystemException — 系统异常基类（RFC 027 M0）
// 对标 C# System.SystemException。
// 所有 CLR / 运行时系统异常的共同基类。
namespace Arc;

/// <summary>
/// 系统异常基类。CLR / 运行时抛出的预定义异常的共同基类。
/// 与 ApplicationException（用户代码异常）区分。
/// </summary>
public class SystemException : Exception {
    public SystemException() : base() { }
    public SystemException(string message) : base(message) { }
    public SystemException(string message, Exception? innerException) : base(message, innerException) { }
}
