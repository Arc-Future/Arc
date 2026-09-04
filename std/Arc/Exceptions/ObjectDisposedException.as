// ObjectDisposedException — 对象已释放异常（RFC 027 M0）
// 对标 C# System.ObjectDisposedException。
namespace Arc;

/// <summary>
/// 对已释放对象执行操作时抛出。
///
/// 常见场景：
///   - 对已 Dispose 的 Stream/Semaphore/Mutex 调用方法
///   - 对已关闭的资源再次读取/写入
/// </summary>
public class ObjectDisposedException : SystemException {
    /// <summary>已释放对象的名称。</summary>
    public string ObjectName { get; }

    /// <param name="objectName">已释放对象的名称。</param>
    public ObjectDisposedException(string objectName) : base(objectName) {
        this.ObjectName = objectName;
    }

    /// <param name="objectName">已释放对象的名称。</param>
    /// <param name="message">自定义错误消息。</param>
    public ObjectDisposedException(string objectName, string message) : base(message) {
        this.ObjectName = objectName;
    }
}
