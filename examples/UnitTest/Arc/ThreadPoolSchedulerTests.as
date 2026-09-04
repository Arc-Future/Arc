namespace UnitTest.Arc;

using Arc;
using Arc.Collections.Concurrent;
using Arc.QIF;
using Arc.Threading;

/// <summary>
/// 显式 ThreadPoolScheduler L2 Stable：基本 API + Destroy + NUMA ctor + 多任务压力最小面。
/// await 混跑 / 协作抢占路径由非 Skip e2e <c>threadpool_scheduler_e2e</c> 压实；
/// C ABI 压力 / NUMA / preempt check 见 <c>threadpool_stress_e2e</c>。
/// </summary>
public class ThreadPoolSchedulerTests
{
    [Fact]
    public void ThreadPoolScheduler_Create_WorkerCount()
    {
        var pool = new ThreadPoolScheduler(2, false);
        Assert.Equal(2, pool.ActiveWorkerCount);
        Assert.Equal(0, pool.PendingTaskCount);
        // H1: Shutdown 后须 Destroy——否则池结构/deque 残留至进程退出。
        pool.Shutdown();
        pool.Destroy();
    }

    /// <summary>RFC 057 M3：零参 ctor 填缺省 workerCount=0 / numaAware=false。</summary>
    [Fact]
    public void ThreadPoolScheduler_Create_Defaults()
    {
        var pool = new ThreadPoolScheduler();
        Assert.True(pool.ActiveWorkerCount > 0);
        Assert.Equal(0, pool.PendingTaskCount);
        pool.Destroy();
    }

    /// <summary>RFC 057 M3：省略 numaAware；命名实参跳过 workerCount。</summary>
    [Fact]
    public void ThreadPoolScheduler_Create_Optional_And_Named()
    {
        var pool = new ThreadPoolScheduler(2);
        Assert.Equal(2, pool.ActiveWorkerCount);
        pool.Destroy();
        var numa = new ThreadPoolScheduler(numaAware: true);
        Assert.True(numa.ActiveWorkerCount > 0);
        numa.Destroy();
    }

    [Fact]
    public void ThreadPoolScheduler_Run_Action_Completes()
    {
        var pool = new ThreadPoolScheduler(2, false);
        var t = pool.Run(() => { });
        t.Wait(5000);
        Assert.True(t.IsCompleted);
        Assert.Equal(0, pool.PendingTaskCount);
        pool.Shutdown();
        pool.Destroy();
    }

    [Fact]
    public void Task_Run_Action_On_Explicit_Pool_Completes()
    {
        var pool = new ThreadPoolScheduler(2, false);
        var t = Task.Run(() => { }, pool);
        t.Wait(5000);
        Assert.True(t.IsCompleted);
        Assert.Equal(2, pool.ActiveWorkerCount);
        pool.Shutdown();
        pool.Destroy();
    }

    [Fact]
    public void ThreadPoolScheduler_Destroy_After_Work()
    {
        var pool = new ThreadPoolScheduler(2, false);
        var t = pool.Run(() => { });
        t.Wait(5000);
        Assert.True(t.IsCompleted);
        pool.Destroy();
    }

    [Fact]
    public void ThreadPoolScheduler_Shutdown_Then_Destroy()
    {
        var pool = new ThreadPoolScheduler(2, false);
        var t = pool.Run(() => { });
        t.Wait(5000);
        Assert.True(t.IsCompleted);
        pool.Shutdown();
        pool.Destroy();
    }

    [Fact]
    public void ThreadPoolScheduler_NumaAware_Create_Run()
    {
        var pool = new ThreadPoolScheduler(2, true);
        Assert.Equal(2, pool.ActiveWorkerCount);
        var t = pool.Run(() => { });
        t.Wait(5000);
        Assert.True(t.IsCompleted);
        pool.Destroy();
    }

    [Fact]
    public void ThreadPoolScheduler_Pressure_Many_Tasks()
    {
        var pool = new ThreadPoolScheduler(4, false);
        var bag = new ConcurrentBag<int>();
        Task t0 = pool.Run(() => { bag.Add(1); });
        Task t1 = pool.Run(() => { bag.Add(1); });
        Task t2 = pool.Run(() => { bag.Add(1); });
        Task t3 = pool.Run(() => { bag.Add(1); });
        Task t4 = pool.Run(() => { bag.Add(1); });
        Task t5 = pool.Run(() => { bag.Add(1); });
        Task t6 = pool.Run(() => { bag.Add(1); });
        Task t7 = pool.Run(() => { bag.Add(1); });
        t0.Wait(5000);
        t1.Wait(5000);
        t2.Wait(5000);
        t3.Wait(5000);
        t4.Wait(5000);
        t5.Wait(5000);
        t6.Wait(5000);
        t7.Wait(5000);
        Assert.Equal(8, bag.Count);
        Assert.Equal(0, pool.PendingTaskCount);
        pool.Destroy();
    }
}
