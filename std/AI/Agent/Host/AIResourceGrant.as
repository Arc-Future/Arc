// RFC 038 §13 / 043 conflict-fabric §3：统一租约授权（AICoordinator.Acquire 签发）。
// HolderId 恒等于 SessionId；TaskRunId 为审计元数据（非第二锁）。
namespace Arc.Agent;

/// <summary>
/// 统一租约授权，由 <see cref="AICoordinator.Acquire"/> 签发。冲突（其它会话已持有
/// 同 Kind 同资源租约）→ <see cref="Acquired"/> = false：后到拒绝、不排队、先到者不受阻。
/// Commit **不**自动放锁——租约持续持有至显式 <see cref="AICoordinator.Release"/> /
/// <see cref="AICoordinator.ReleaseSession"/>，否则单次提交即放锁会让其它会话趁编辑间隙
/// 取得租约，破坏冲突保护。
/// </summary>
public class AIResourceGrant {
    /// <summary>租约种类（ToolPath / Plan / RfcSpec）。</summary>
    public AILeaseKind Kind;
    /// <summary>持有者（恒等于 SessionId；跨会话仲裁按此键）。</summary>
    public string HolderId;
    /// <summary>会话 id（与 HolderId 同值；消费面可读语义）。</summary>
    public string SessionId;
    /// <summary>可选任务运行 id（审计元数据，非第二锁）。</summary>
    public string? TaskRunId;
    /// <summary>登记表资源键：ToolPath 为规范路径；Plan = "plan:"+PlanId；RfcSpec = "airfc:"+RfcId。</summary>
    public string Key;
    /// <summary>是否成功获取租约；false = 冲突被拒（后到拒绝）。</summary>
    public bool Acquired;
    /// <summary>ToolPath 特化：显式覆写确权（CommitAsync 据此记录覆写审计）。</summary>
    public bool IsOverwrite;

    public AIResourceGrant(AILeaseKind kind, string holderId, string? taskRunId, string key, bool acquired, bool isOverwrite) {
        this.Kind = kind;
        this.HolderId = holderId != null ? holderId : "";
        this.SessionId = this.HolderId;
        this.TaskRunId = taskRunId;
        this.Key = key != null ? key : "";
        this.Acquired = acquired;
        this.IsOverwrite = isOverwrite;
    }
}
