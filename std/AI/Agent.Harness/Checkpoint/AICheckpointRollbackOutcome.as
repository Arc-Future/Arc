// RFC 043 P2：回滚结果 — 恢复/删除/跳过计数 + 目标绿点 + 联动摘要 + 成败。
namespace Arc.Agent.Harness;

/// <summary>
/// 绿点回滚执行结果（Detail 回喂模型/升级人）。携带目标绿点 id 与快照内嵌的 AIRfc
/// Revision / AIPlan 状态摘要，供 Harness 层做回滚联动。
/// </summary>
public class AICheckpointRollbackOutcome {
    public bool FoundSnapshot;
    public bool Success;
    public int RestoredCount;
    public int DeletedCount;
    public int SkippedCount;
    /// <summary>回滚目标绿点 id（如 "cp-000001"）；无快照时为空。</summary>
    public string CheckpointId;
    /// <summary>目标绿点记录（捕获时点）的 AIRfc Revision。</summary>
    public int RfcRevision;
    /// <summary>目标绿点记录的 AIPlan 状态摘要（可空字符串）。</summary>
    public string PlanStatusSummary;
    public string Detail;

    public AICheckpointRollbackOutcome() {
        this.FoundSnapshot = false;
        this.Success = false;
        this.RestoredCount = 0;
        this.DeletedCount = 0;
        this.SkippedCount = 0;
        this.CheckpointId = "";
        this.RfcRevision = 0;
        this.PlanStatusSummary = "";
        this.Detail = "";
    }

    /// <summary>回滚摘要（checkpoint:rollback 事件 Detail 折叠；携带绿点 id / 版本）。</summary>
    public string Describe() {
        if (!this.FoundSnapshot) {
            return "rollback: no snapshot (escalate)";
        }
        return "rollback: cp=" + (this.CheckpointId != "" ? this.CheckpointId : "?")
            + " rfc:v" + this.RfcRevision
            + " restored=" + this.RestoredCount
            + ", deleted=" + this.DeletedCount
            + ", skipped=" + this.SkippedCount
            + (this.Success ? "" : ", partial");
    }
}
