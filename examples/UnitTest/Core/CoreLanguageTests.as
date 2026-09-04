namespace UnitTest.Core;

using Arc;
using Arc.QIF;

/// <summary>
/// CoreLanguage 基础语法测试：覆盖 CoreLanguage / Hello / ControlFlow / IfTest 示例程序中的逻辑验证。
/// </summary>
public class CoreLanguageTests
{
    // ── CoreLanguage: Add ──

    [Fact]
    public void Add_TwoPositiveNumbers()
    {
        int a = 3;
        int b = 4;
        int sum = a + b;
        Assert.Equal(7, sum);
    }

    [Fact]
    public void Add_SumGreaterThanSix()
    {
        int a = 3;
        int b = 4;
        int sum = a + b;
        Assert.Greater(sum, 6);
    }

    // ── CoreLanguage: Factorial ──

    [Fact]
    public void Factorial_OfFive()
    {
        int n = 5;
        int result = 1;
        int i = 1;
        while (i <= n) {
            result = result * i;
            i = i + 1;
        }
        Assert.Equal(120, result);
    }

    // ── Hello ──

    [Fact]
    public void Hello_WorldString()
    {
        string greeting = "Hello, World!";
        Assert.True(greeting == "Hello, World!");
    }

    // ── ControlFlow: int branching (mirrors original switch) ──

    [Fact]
    public void IntBranch_Zero()
    {
        int code = 0;
        string name = "";
        if (code == 0) {
            name = "zero";
        }
        Assert.True(name == "zero");
    }

    [Fact]
    public void IntBranch_One()
    {
        int code = 1;
        string name = "";
        if (code == 1) {
            name = "one";
        }
        Assert.True(name == "one");
    }

    [Fact]
    public void IntBranch_Other()
    {
        int code = 99;
        bool isOther = code != 0 && code != 1;
        Assert.True(isOther);
    }

    // ── ControlFlow: enum branching (mirrors original switch) ──

    [Fact]
    public void EnumBranch_Running()
    {
        JobStatus status = JobStatus.Running;
        Assert.True(status == JobStatus.Running);
    }

    [Fact]
    public void EnumBranch_Idle()
    {
        JobStatus status = JobStatus.Idle;
        Assert.True(status == JobStatus.Idle);
    }

    [Fact]
    public void EnumBranch_Done()
    {
        JobStatus status = JobStatus.Done;
        Assert.True(status == JobStatus.Done);
    }

    // ── IfTest: if / else ──

    [Fact]
    public void If_GreaterThan_True()
    {
        int x = 7;
        int y = 6;
        Assert.True(x > y);
    }

    [Fact]
    public void If_GreaterThan_False()
    {
        int x = 3;
        int y = 6;
        Assert.False(x > y);
    }
}

enum JobStatus {
    Idle,
    Running,
    Done,
}
