namespace UnitTest.Arc;

using Arc;
using Arc.QIF;

/// <summary>
/// Task 同步 API 契约（L2 Stable）：FromResult / FromCanceled / FromException /
/// CompletedTask / WhenAll·WhenAny·WaitAll·WaitAny（`params ReadOnlySpan&lt;Task&gt;`）/
/// Cancel / <c>Task.Run</c>（默认线程池 Action / Func&lt;T&gt;）。
/// <c>Yield</c> 已撤面（调度器让步 ABI 后置）。异步路径：QIF <c>[Fact] async</c>
/// 见 <c>EventLoopTests</c>；CancelAfter 等由 <c>cancellation_e2e</c> /
/// <c>event_loop_e2e</c> / <c>async_tasks_e2e</c> / <c>task_run_e2e</c> 非 Skip 压实。
/// </summary>
public class TaskTests
{
    // ── Task.FromResult ──

    [Fact]
    public void Task_FromResult_Int()
    {
        var t = Task.FromResult(42);
        Assert.True(t.IsCompleted);
        Assert.Equal(42, t.Result);
    }

    [Fact]
    public void Task_FromResult_String()
    {
        var t = Task.FromResult("hello");
        Assert.True(t.IsCompleted);
        Assert.True(t.Result == "hello");
    }

    // ── Task 属性 / GetResult ──

    [Fact]
    public void Task_IsCompleted_True()
    {
        var t = Task.FromResult(1);
        Assert.True(t.IsCompleted);
    }

    [Fact]
    public void Task_GetResult_Method()
    {
        var t = Task.FromResult(99);
        var result = t.GetResult();
        Assert.Equal(99, result);
    }

    // ── CompletedTask / WhenAll（同步已完成 inner）──

    [Fact]
    public void Task_CompletedTask_IsCompleted()
    {
        Task t = Task.CompletedTask;
        Assert.True(t.IsCompleted);
    }

    [Fact]
    public void Task_WhenAll_CompletedTasks()
    {
        Task t1 = Task.CompletedTask;
        Task t2 = Task.CompletedTask;
        Task all = Task.WhenAll(t1, t2);
        Assert.True(all.IsCompleted);
    }

    [Fact]
    public void Task_WhenAll_Empty()
    {
        Task all = Task.WhenAll();
        Assert.True(all.IsCompleted);
    }

    [Fact]
    public void Task_WhenAny_CompletedTasks()
    {
        Task t1 = Task.CompletedTask;
        Task t2 = Task.CompletedTask;
        Task any = Task.WhenAny(t1, t2);
        Assert.True(any.IsCompleted);
    }

    [Fact]
    public void Task_WaitAll_CompletedTasks()
    {
        Task t1 = Task.CompletedTask;
        Task t2 = Task.CompletedTask;
        Task.WaitAll(t1, t2);
        Assert.True(t1.IsCompleted);
        Assert.True(t2.IsCompleted);
    }

    [Fact]
    public void Task_WaitAny_CompletedTasks()
    {
        Task t1 = Task.CompletedTask;
        Task t2 = Task.CompletedTask;
        int idx = Task.WaitAny(t1, t2);
        Assert.True(idx >= 0);
        Assert.True(idx <= 1);
    }

    // ── Cancel / FromCanceled ──

    [Fact]
    public void Task_Cancel_SetsIsCanceled()
    {
        // 不用缓存单例值（int∈[0,255]）：rt_task.c 的 RT_TASK_FROM_CACHE 哨兵守卫使缓存
        // 单例不可取消（保持 READY，对标 .NET 对已完成 Task 抛 IAE；task_slab_e2e 压实）。
        // 用缓存范围外的值（非单例、可取消）验证 Cancel 契约本身。
        var t = Task.FromResult(300);
        t.Cancel();
        Assert.True(t.IsCanceled);
    }

    [Fact]
    public void Task_FromCanceled_IsCanceled()
    {
        var cts = new CancellationTokenSource();
        cts.Cancel();
        Task t = Task.FromCanceled(cts.Token);
        Assert.True(t.IsCanceled);
    }

    [Fact]
    public void Task_FromCanceled_Generic_IsCanceled()
    {
        // 显式泛型实参形态：Task.FromCanceled<T>(token) 经 MethodCall.type_args 落为 Task<T>
        var cts = new CancellationTokenSource();
        cts.Cancel();
        Task<int> t = Task.FromCanceled<int>(cts.Token);
        Assert.True(t.IsCanceled);
    }

    [Fact]
    public void Task_FromException_IsFaulted()
    {
        Exception boom = new Exception("boom");
        Task t = Task.FromException(boom);
        Assert.True(t.IsFaulted);
        Assert.False(t.IsCanceled);
        Assert.NotNull(t.Exception);
        Assert.Equal("boom", t.Exception.Message);
    }

    // ── Task.Run（默认线程池 · L2 最小可宣称）──
    // Func&lt;T&gt; 结果路径由非 Skip e2e `task_run_e2e` + EventLoopTests async Fact 压实；
    // 此处覆盖 Action 完成契约（避免 sync Result 在 QIF 宿主上的 expected-ty 分派歧义）。

    [Fact]
    public void Task_Run_Action_Completes()
    {
        var t = Task.Run(() => { });
        t.Wait(5000);
        Assert.True(t.IsCompleted);
    }
}
