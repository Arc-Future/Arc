namespace UnitTest.Arc;

using Arc;
using Arc.QIF;

/// <summary>
/// Task 状态枚举与 CompletedTask 补充（与 TaskTests 同步面互补）。
/// 使用 std <c>Arc.TaskStatus</c>，禁止本地同名 enum 双轨遮蔽。
/// </summary>
public class AsyncTaskTests
{
    [Fact]
    public void Task_Status_FromResult_IsReady()
    {
        Task<int> t = Task.FromResult(42);
        Assert.Equal((int)TaskStatus.Ready, (int)t.Status);
    }
}
