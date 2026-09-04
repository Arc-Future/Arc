namespace UnitTest.Arc;

using Arc;
using Arc.QIF;

/// <summary>
/// CancellationToken 单元测试：覆盖 Cancellation 示例的同步 API 表面。
/// 验证 CTS 创建、Cancel、Dispose 等不需异步运行时的部分。
/// 异步取消行为（CancelAfter、Task.Delay+ct）需 EventLoop。
/// </summary>
public class CancellationTests
{
    // ── CancellationTokenSource 构造 ──

    [Fact]
    public void CTS_Create_Succeeds()
    {
        CancellationTokenSource cts = new CancellationTokenSource();
        Assert.True(true);
    }

    // ── CTS.Token ──

    [Fact]
    public void CTS_Token_Accessible()
    {
        CancellationTokenSource cts = new CancellationTokenSource();
        CancellationToken ct = cts.Token;
        Assert.True(true);
    }

    // ── CTS.Cancel ──

    [Fact]
    public void CTS_Cancel_SetsIsCancelled()
    {
        CancellationTokenSource cts = new CancellationTokenSource();
        cts.Cancel();
        Assert.True(cts.IsCancellationRequested);
    }

    // ── CTS.Dispose ──

    [Fact]
    public void CTS_Dispose_NoException()
    {
        CancellationTokenSource cts = new CancellationTokenSource();
        cts.Dispose();
        Assert.True(true);
    }
}
