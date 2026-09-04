namespace UnitTest.Arc;

using Arc;
using Arc.QIF;

/// <summary>
/// EventLoop / QIF <c>[Fact] async Task</c> Stable 面：宿主对 <c>is_async</c> 生成
/// <c>await</c>，由 async Main → EventLoop 驱动 Delay 与跨 await local。
/// 最小契约亦由非 Skip e2e 压实：<c>event_loop_e2e</c>、<c>async_tasks_e2e</c>。
/// </summary>
public class EventLoopTests
{
    [Fact]
    public async Task AsyncFact_FromResult_42()
    {
        var t = Task.FromResult(42);
        Assert.Equal(42, t.Result);
    }

    [Fact]
    public async Task AsyncFact_Delay_10ms()
    {
        await Task.Delay(10);
        Assert.True(true);
    }

    [Fact]
    public async Task AsyncFact_CrossAwait_LocalSurvives()
    {
        var x = 100;
        await Task.Delay(5);
        Assert.Equal(100, x);
    }
}
