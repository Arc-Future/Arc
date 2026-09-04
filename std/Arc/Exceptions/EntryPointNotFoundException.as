// EntryPointNotFoundException — 入口点未找到异常（RFC 027 M0）
// 对标 C# System.EntryPointNotFoundException。
namespace Arc;

/// <summary>
/// 未找到程序入口点时抛出。
/// </summary>
public class EntryPointNotFoundException : SystemException {
    public EntryPointNotFoundException(string message) : base(message) { }
}
