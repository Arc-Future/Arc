namespace UnitTest.Arc;

using Arc;
using Arc.Diagnostics;
using Arc.QIF;
using Arc.Threading;

/// <summary>
/// Threading 最小可绿切片：Mutex、lock 糖、Interlocked int 面、Thread.Sleep 诚实。
/// </summary>
public class ThreadingTests
{
    [Fact]
    public void Mutex_CreateAndRelease()
    {
        Mutex m = new Mutex();
        m.Lock();
        m.Unlock();
        Assert.True(true);
    }

    [Fact]
    public void Mutex_TryLock()
    {
        Mutex m = new Mutex();
        Assert.True(m.TryLock());
        Assert.False(m.TryLock());
        m.Unlock();
        Assert.True(m.TryLock());
        m.Unlock();
    }

    [Fact]
    public void Mutex_CreateDefault()
    {
        Mutex m = new Mutex();
        Assert.True(true);
    }

    [Fact]
    public void Thread_Sleep_Zero()
    {
        Thread.Sleep(0);
        Assert.True(true);
    }

    [Fact]
    public void Thread_Sleep_Positive_AdvancesStopwatch()
    {
        Stopwatch sw = Stopwatch.StartNew();
        Thread.Sleep(20);
        sw.Stop();
        Assert.True(sw.ElapsedMilliseconds >= 10);
    }

    [Fact]
    public void Thread_CurrentThread_HasId()
    {
        int id = Thread.ManagedThreadId;
        Assert.Greater(id, 0);
    }

    [Fact]
    public void Lock_Statement_EnterExit()
    {
        Lock l = new Lock();
        int n = 0;
        lock (l) {
            n = n + 1;
        }
        Assert.Equal(1, n);
    }

    [Fact]
    public void Interlocked_Increment_ReturnsNew()
    {
        int x = 10;
        int n = Interlocked.Increment(ref x);
        Assert.Equal(11, n);
        Assert.Equal(11, x);
    }

    [Fact]
    public void Interlocked_Exchange_ReturnsOld()
    {
        int x = 7;
        int old = Interlocked.Exchange(ref x, 99);
        Assert.Equal(7, old);
        Assert.Equal(99, x);
    }

    [Fact]
    public void Interlocked_CompareExchange_MatchAndMiss()
    {
        int x = 5;
        int hit = Interlocked.CompareExchange(ref x, 50, 5);
        Assert.Equal(5, hit);
        Assert.Equal(50, x);
        int miss = Interlocked.CompareExchange(ref x, 1, 5);
        Assert.Equal(50, miss);
        Assert.Equal(50, x);
    }

    [Fact]
    public void Interlocked_Decrement_ReturnsNew()
    {
        int x = 10;
        int n = Interlocked.Decrement(ref x);
        Assert.Equal(9, n);
        Assert.Equal(9, x);
    }

    // ── Semaphore 计数边界（确定性：用带超时 Wait 断言阻塞/放行，不依赖时序）──

    [Fact]
    public void Semaphore_CountsDown_BlocksAtZero()
    {
        Semaphore sem = new Semaphore(1, 1);
        Assert.True(sem.Wait(50));   // 消耗唯一许可
        Assert.False(sem.Wait(20));  // 计数已空 → 超时
        sem.Release();               // 归还许可
        Assert.True(sem.Wait(50));   // 再次可获取
        sem.Release();
    }

    [Fact]
    public void Semaphore_InitialZero_ReleaseGrants()
    {
        Semaphore sem = new Semaphore(0, 1);
        Assert.False(sem.Wait(20));  // 初始 0 → 超时
        sem.Release();               // 计数 0 → 1
        Assert.True(sem.Wait(50));
        sem.Release();
    }

    [Fact]
    public void Semaphore_MultiplePermits()
    {
        Semaphore sem = new Semaphore(3, 3);
        Assert.True(sem.Wait(50));
        Assert.True(sem.Wait(50));
        Assert.True(sem.Wait(50));
        Assert.False(sem.Wait(20));  // 3 个许可全部消耗
        sem.Release();
        Assert.True(sem.Wait(50));
        sem.Release();
        sem.Release();
        sem.Release();
    }

    // ── TaskCompletionSource 显式完成（RFC 008；对象即 PENDING 态 RtTask*）──

    [Fact]
    public void Tcs_SetResult_CompletesTask()
    {
        TaskCompletionSource<int> tcs = new TaskCompletionSource<int>();
        Task<int> t = tcs.Task;
        Assert.False(t.IsCompleted);
        tcs.SetResult(42);
        Assert.True(t.IsCompleted);
        Assert.False(t.IsCanceled);
        Assert.Equal(42, t.Result);
    }

    [Fact]
    public void Tcs_SetCanceled_IsCanceled()
    {
        TaskCompletionSource<int> tcs = new TaskCompletionSource<int>();
        Task<int> t = tcs.Task;
        tcs.SetCanceled();
        Assert.True(t.IsCanceled);
    }

    [Fact]
    public void Tcs_SetException_IsFaulted()
    {
        TaskCompletionSource<int> tcs = new TaskCompletionSource<int>();
        Task<int> t = tcs.Task;
        Exception boom = new Exception("tcs-boom");
        tcs.SetException(boom);
        Assert.True(t.IsFaulted);
        Assert.NotNull(t.Exception);
        Assert.Equal("tcs-boom", t.Exception.Message);
    }
}
