namespace UnitTest.Core;

using Arc;
using Arc.QIF;

/// <summary>
/// Async 状态机 lowering（RFC 029 M2）：多 await、跨 await local、混合类型。
/// </summary>
public class AsyncStateMachineTests
{
    public async Task<string> GetName() { return "Alice"; }
    public async Task<int> GetAge() { return 18; }
    public async Task<int> GetScore() { return 95; }
    public async Task<int> FetchValue() { return 42; }
    public async Task<int> GetFirst() { return 100; }
    public async Task<int> GetSecond() { return 200; }

    [Fact]
    public async Task MultiAwait_LocalsSurvive()
    {
        string name = await this.GetName();
        int age = await this.GetAge();
        int score = await this.GetScore();
        Assert.Equal("Alice", name);
        Assert.Equal(18, age);
        Assert.Equal(95, score);
    }

    [Fact]
    public async Task SingleAwait_ReadyTask()
    {
        int value = await this.FetchValue();
        Assert.Equal(42, value);
    }

    [Fact]
    public async Task CrossAwait_LocalSum()
    {
        int first = await this.GetFirst();
        int second = await this.GetSecond();
        Assert.Equal(300, first + second);
    }
}
