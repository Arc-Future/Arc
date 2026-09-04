// RFC 043 场景 2.3/4.3：L2 修复循环结果 — Passed（收敛 + 轮数）/ NeedsHuman（超限回滚 + 升级，
// 或无绿点前置提示 / 无修复提供者）。BudgetExceeded / RolledBack 语义显式暴露供调用方决策。
namespace Arc.Agent.Harness;
using Arc;
using Arc.Collections;

/// <summary>
/// <see cref="AIDoDOrchestrator.RunFixLoopAsync"/> 的结果：自动迭代收敛（Passed + 轮数）
/// 或升级人（NeedsHuman；超限回滚由 <see cref="AIHarnessSession"/> 层触发后置
/// <see cref="RolledBack"/>）。
/// </summary>
public class AIDoDFixLoopResult {
    /// <summary>Passed = 收敛；NeedsHuman = 升级人（超限 / 无绿点 / 无修复提供者）。</summary>
    public AIDoDGateStatus Status;
    /// <summary>已消耗修复轮数（RecordFixAttempt 计数；Passed 时 = 收敛所需轮数）。</summary>
    public int FixRounds;
    /// <summary>是否超限（FixBudgetExceeded 语义：达到 maxRounds 仍失败）。</summary>
    public bool BudgetExceeded;
    /// <summary>超限后是否已回滚最近绿点（AIHarnessSession 层执行 CheckpointRollbackAsync 后置位）。</summary>
    public bool RolledBack;
    /// <summary>升级原因 / 前置提示（如「无绿点先 /checkpoint」「无修复提供者」）。</summary>
    public string Reason;
    /// <summary>最终一轮门结果（收敛 = 全 Passed；超限 = 最后失败门）。</summary>
    public List<AIDoDGateResult> Gates;
    /// <summary>最后一轮失败回喂（诊断明细）；收敛或无失败时为 null。</summary>
    public AIDoDFixFeedback? Feedback;

    public AIDoDFixLoopResult() {
        this.Status = AIDoDGateStatus.NeedsHuman;
        this.FixRounds = 0;
        this.BudgetExceeded = false;
        this.RolledBack = false;
        this.Reason = "";
        this.Gates = new List<AIDoDGateResult>();
        this.Feedback = null;
    }

    public bool IsPassed {
        get { return this.Status == AIDoDGateStatus.Passed; }
    }

    public static AIDoDFixLoopResult Passed(
        AIDoDGateKind gate,
        List<AIDoDGateResult> gates,
        int fixRounds) {
        AIDoDFixLoopResult r = new AIDoDFixLoopResult();
        r.Status = AIDoDGateStatus.Passed;
        r.FixRounds = fixRounds;
        r.BudgetExceeded = false;
        r.RolledBack = false;
        r.Reason = "iterated to green";
        r.Gates = AIDoDFixLoopResult.Copy(gates);
        return r;
    }

    public static AIDoDFixLoopResult BudgetExceeded(
        AIDoDGateKind gate,
        List<AIDoDGateResult> gates,
        AIDoDFixFeedback? feedback,
        int fixRounds) {
        AIDoDFixLoopResult r = new AIDoDFixLoopResult();
        r.Status = AIDoDGateStatus.NeedsHuman;
        r.BudgetExceeded = true;
        r.FixRounds = fixRounds;
        r.Reason = "fix budget exceeded";
        r.Gates = AIDoDFixLoopResult.Copy(gates);
        r.Feedback = feedback;
        return r;
    }

    public static AIDoDFixLoopResult NoProvider(
        AIDoDGateKind gate,
        List<AIDoDGateResult> gates,
        AIDoDFixFeedback feedback) {
        AIDoDFixLoopResult r = new AIDoDFixLoopResult();
        r.Status = AIDoDGateStatus.NeedsHuman;
        r.BudgetExceeded = false;
        r.Reason = "no fix round provider wired — cannot auto-iterate; escalate to human";
        r.Gates = AIDoDFixLoopResult.Copy(gates);
        r.Feedback = feedback;
        return r;
    }

    public static AIDoDFixLoopResult Escalated(
        AIDoDGateKind gate,
        List<AIDoDGateResult> gates,
        AIDoDFixFeedback? feedback,
        int fixRounds,
        string reason,
        bool budgetExceeded,
        bool rolledBack) {
        AIDoDFixLoopResult r = new AIDoDFixLoopResult();
        r.Status = AIDoDGateStatus.NeedsHuman;
        r.FixRounds = fixRounds;
        r.BudgetExceeded = budgetExceeded;
        r.RolledBack = rolledBack;
        r.Reason = reason != null ? reason : "";
        r.Gates = AIDoDFixLoopResult.Copy(gates);
        r.Feedback = feedback;
        return r;
    }

    private static List<AIDoDGateResult> Copy(List<AIDoDGateResult> gates) {
        List<AIDoDGateResult> outList = new List<AIDoDGateResult>();
        if (gates != null) {
            int i = 0;
            int n = gates.Count;
            while (i < n) {
                outList.Add(gates[i]);
                i = i + 1;
            }
        }
        return outList;
    }
}
