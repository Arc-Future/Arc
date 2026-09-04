namespace UnitTest.Arc;

using Arc;
using Arc.QIF;
using Arc.Threading;

/// <summary>
/// Parallel / Threading 扩展测试：在 ThreadingTests.as 基础上补充
/// Semaphore、Monitor、Lock 等同步原语的 API 表面。
/// 注：Parallel.ForAsync 需要线程池运行时，此处覆盖同步 API。
/// </summary>
public class ParallelTests
{
    // ── Semaphore ──

    [Fact]
    public void Semaphore_Create()
    {
        Semaphore s = new Semaphore(1, 1);
        Assert.True(true);
    }

    [Fact]
    public void Semaphore_WaitRelease()
    {
        Semaphore s = new Semaphore(1, 1);
        s.Wait();
        s.Release();
        Assert.True(true);
    }

    // ── Monitor ──

    [Fact]
    public void Monitor_EnterExit()
    {
        Lock lockObj = new Lock();
        Monitor.Enter(lockObj);
        Monitor.Exit(lockObj);
        Assert.True(true);
    }

    [Fact]
    public void Monitor_TryEnter()
    {
        Lock lockObj = new Lock();
        Assert.True(Monitor.TryEnter(lockObj));
        // Monitor/Lock 对齐 C#：同线程可重入；不测二次 TryEnter 失败。
        Monitor.Exit(lockObj);
        Assert.True(Monitor.TryEnter(lockObj));
        Monitor.Exit(lockObj);
    }

    // ── Lock ──

    [Fact]
    public void Lock_Create()
    {
        Lock l = new Lock();
        Assert.True(true);
    }

    // ── lock 语句糖（RFC 029 §7.3）──

    [Fact]
    public void Lock_Statement_CriticalSection()
    {
        Lock l = new Lock();
        int x = 0;
        lock (l) {
            x = 42;
        }
        Assert.Equal(42, x);
    }
}
