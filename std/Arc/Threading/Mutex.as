// Mutex — 非递归互斥锁（RFC 009 §7.2 / M5.5）
namespace Arc.Threading {

/// <summary>
/// 非递归互斥锁。与 POSIX pthread_mutex 默认非递归语义一致，
/// 避免 C# Mutex（Win32 递归 mutex）的误用风险；递归需求用 Monitor。
/// Windows 实现为 SRWLOCK 独占（不可重入）；POSIX 为默认 mutex。
///
/// 此声明为 stub，方法体不执行；codegen 拦截后直接发射 rt_mutex_* ABI。
/// new() 在 emit_new.rs 拦截为 @rt_mutex_create()，实例方法走 receiver_type 匹配。
/// </summary>
public class Mutex : Arc.IDisposable {
    /// <summary>获取互斥锁（阻塞直到可用）。</summary>
    [Builtin(ABI = "rt_mutex_lock")]
    public void Lock() {}
    /// <summary>尝试获取互斥锁（非阻塞）。</summary>
    /// <returns>true 表示成功获取。</returns>
    [Builtin(ABI = "rt_mutex_try_lock")]
    public bool TryLock() { return false; }
    /// <summary>释放互斥锁。</summary>
    [Builtin(ABI = "rt_mutex_unlock")]
    public void Unlock() {}
    /// <summary>释放互斥锁资源。</summary>
    [Builtin(ABI = "rt_mutex_destroy")]
    public void Dispose() {}
}

} // namespace Arc.Threading
