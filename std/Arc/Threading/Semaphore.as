// Semaphore — 计数信号量（RFC 009 §7.2 / M5.5）
namespace Arc.Threading {

/// <summary>
/// 计数信号量。控制对资源池的并发访问（最多 maximum 个并发持有者）。
///
/// [Builtin] 方法为 codegen stub，body 不执行。无 [Builtin] 的异步方法
/// 通过 Task.Run 包装同步操作，提供 async 接口。
/// </summary>
public class Semaphore : Arc.IDisposable {
    /// <summary>创建信号量。</summary>
    [Builtin(ABI = "rt_semaphore_create")]
    public Semaphore(int initial, int maximum) {}

    /// <summary>等待信号量（阻塞直到可用）。</summary>
    [Builtin(ABI = "rt_semaphore_wait")]
    public void Wait() {}
    /// <summary>等待信号量，超时返回。</summary>
    [Builtin(ABI = "rt_semaphore_wait")]
    public bool Wait(int milliseconds) { return false; }
    /// <summary>释放信号量（计数 +1）。</summary>
    [Builtin(ABI = "rt_semaphore_release")]
    public void Release() {}
    /// <summary>释放信号量（计数 +count，批量归还；count &lt;= 0 为 no-op）。</summary>
    [Builtin(ABI = "rt_semaphore_release_n")]
    public void Release(int count) {}
    /// <summary>释放信号量资源。</summary>
    [Builtin(ABI = "rt_semaphore_destroy")]
    public void Dispose() {}

    // ---- Async 方法（纯 Arc 组合，通过 Task.Run 包装同步 Wait）----

    /// <summary>异步等待信号量。</summary>
    public async Task WaitAsync(CancellationToken cancellationToken = default) {
        cancellationToken.ThrowIfCancellationRequested();
        await Task.Run(() => this.Wait());
    }

    /// <summary>异步等待信号量，超时返回。</summary>
    /// <returns>true 表示成功获取；false 表示超时。</returns>
    public async Task<bool> WaitAsync(int millisecondsTimeout, CancellationToken cancellationToken = default) {
        cancellationToken.ThrowIfCancellationRequested();
        return await Task.Run<bool>(() => this.Wait(millisecondsTimeout));
    }
}

} // namespace Arc.Threading
