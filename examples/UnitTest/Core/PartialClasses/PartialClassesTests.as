namespace UnitTest.Core;

using Arc;
using Arc.QIF;
using UnitTest.Core.Partial;

/// <summary>
/// 分部类单元测试：覆盖 PartialClasses 示例的 RFC 037 M1 跨文件合并。
/// 验证两个 partial class 声明在跨文件情况下正确合并，包括跨文件私有字段访问。
/// </summary>
public class PartialClassesTests
{
    // ── 跨文件合并：用户代码字段 + 方法 ──

    [Fact]
    public void PartialClass_UserCode_Properties()
    {
        Counter c = new Counter();
        Assert.Equal(0, c.Count);
        c.Increment();
        c.Increment();
        Assert.Equal(2, c.Count);
    }

    [Fact]
    public void PartialClass_UserCode_Decrement()
    {
        Counter c = new Counter();
        c.Increment();
        c.Increment();
        c.Increment();
        c.Decrement();
        Assert.Equal(2, c.Count);
    }

    // ── 跨文件合并：生成代码字段 + 方法（访问另一文件的 _count 私有字段）──

    [Fact]
    public void PartialClass_GeneratedCode_Reset()
    {
        Counter c = new Counter();
        c.Increment();
        c.Increment();
        Assert.Equal(2, c.Count);

        c.Reset();
        Assert.Equal(0, c.Count);
    }

    [Fact]
    public void PartialClass_GeneratedCode_IsFull()
    {
        Counter c = new Counter();
        Assert.False(c.IsFull);
    }

    // ── 跨文件合并：两边代码同时存在 ──

    [Fact]
    public void PartialClass_Combined_MultipleOps()
    {
        Counter c = new Counter();
        c.Increment();
        c.Increment();
        bool beforeReset = c.IsFull;
        Assert.False(beforeReset);

        c.Reset();
        Assert.Equal(0, c.Count);
    }
}
