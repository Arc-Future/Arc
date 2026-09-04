namespace Arc;

/// 资源释放接口——对标 C# System.IDisposable。
///
/// `using` 语句要求资源类型实现本接口；
/// `using (T r = expr) { ... }` 脱糖为 `try { ... } finally { r.Dispose(); }`。
public interface IDisposable {
    /// <summary>释放资源，由 using 语句在 finally 块中调用。</summary>
    void Dispose();
}
