namespace UnitTest.AI;

using Arc;
using Arc.Agent;
using Arc.QIF;

/// <summary>
/// HITL 门闩 + Wiki Snapshot（非 Skip）。
/// 关闭路径调用 Approve/Reject/ProvideInput（返回已完成 Task）；断言 LastOutcome。
/// Session 接线原由 arc-integration e2e 压实（该 crate 已退场，a2627a0f）。
/// </summary>
public class AISessionHitlTests
{
    [Fact]
    public void Gate_Approve_Edit()
    {
        AIHumanGate gate = new AIHumanGate();
        AIHumanRequest req = new AIHumanRequest("require-approval", "Confirm?");
        req.ToolCallId = "c1";
        req.ToolName = "fs_write";
        req.ToolArguments = "{\"path\":\"a.txt\"}";
        gate.EnterAwaiting(req);
        Assert.True(gate.IsAwaiting);

        CancellationTokenSource cts = new CancellationTokenSource();
        Task t = gate.ApproveAsync("fs_write", "{\"path\":\"b.txt\"}", cts.Token);
        Assert.True(t.IsCompleted);
        Assert.False(gate.IsAwaiting);
        Assert.Null(gate.PendingHuman);
        AIHumanOutcome outcome = gate.LastOutcome;
        Assert.NotNull(outcome);
        Assert.True(outcome.Decision == AIHumanDecision.Approved);
        Assert.Equal("fs_write", outcome.EditedToolName);
        Assert.Equal("{\"path\":\"b.txt\"}", outcome.EditedToolArguments);
    }

    [Fact]
    public void Gate_Reject()
    {
        AIHumanGate gate = new AIHumanGate();
        gate.EnterAwaiting(new AIHumanRequest("policy", "Deny?"));
        CancellationTokenSource cts = new CancellationTokenSource();
        Task t = gate.RejectAsync("user denied", cts.Token);
        Assert.True(t.IsCompleted);
        AIHumanOutcome outcome = gate.LastOutcome;
        Assert.NotNull(outcome);
        Assert.True(outcome.Decision == AIHumanDecision.Rejected);
        Assert.Equal("user denied", outcome.RejectReason);
        Assert.False(gate.IsAwaiting);
    }

    [Fact]
    public void Gate_ProvideInput()
    {
        AIHumanGate gate = new AIHumanGate();
        gate.EnterAwaiting(new AIHumanRequest("need-input", "Which?"));
        CancellationTokenSource cts = new CancellationTokenSource();
        Task t = gate.ProvideInputAsync("battery-0", cts.Token);
        Assert.True(t.IsCompleted);
        AIHumanOutcome outcome = gate.LastOutcome;
        Assert.NotNull(outcome);
        Assert.True(outcome.Decision == AIHumanDecision.InputProvided);
        Assert.Equal("battery-0", outcome.InputText);
    }

    [Fact]
    public void Wiki_Snapshot_Restore()
    {
        AIWiki wiki = new AIWiki();
        wiki.Upsert("a/one", "1");
        wiki.Upsert("a/two", "2");
        AIWikiSnapshot snap = wiki.CreateSnapshot();
        Assert.Equal(2, snap.Count);
        wiki.Delete("a/one");
        wiki.Restore(snap);
        Assert.NotNull(wiki.Get("a/one"));
        Assert.Equal("1", wiki.Get("a/one").Body);
    }
}
