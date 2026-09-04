namespace UnitTest.QIF;

using Arc;
using Arc.QIF;

/// <summary>
/// QIF 测试生命周期自测：类级构造注入（IQIFOutput）+ IQIFSetup/IQIFTeardown。
/// 验证 host 对每个 [Fact]：先执行构造注入 → 调用 Setup → 执行测试体 → 调用 Teardown。
/// 默认 max_parallel=1（顺序执行），故静态计数可确定性验证调用序。
/// </summary>
public class LifecycleTests : IQIFSetup, IQIFTeardown
{
    private static int _setupCount = 0;
    private static int _teardownCount = 0;
    private IQIFOutput _output;

    public LifecycleTests(IQIFOutput output)
    {
        _output = output;
    }

    public void Setup()
    {
        _setupCount = _setupCount + 1;
    }

    public void Teardown()
    {
        _teardownCount = _teardownCount + 1;
    }

    [Fact]
    public void Ctor_Injects_IQIFOutput()
    {
        Assert.NotNull(_output);
    }

    [Fact]
    public void Setup_Runs_Before_Test()
    {
        // Setup 在本测试前已执行：计数至少为 1。
        Assert.Greater(_setupCount, 0);
    }

    [Fact]
    public void Teardown_Runs_After_Test()
    {
        // 前序测试已完整执行（Setup→测试→Teardown）：Teardown 计数非零。
        Assert.Greater(_teardownCount, 0);
    }
}
