// RFC 043 subagent-management §4 / §8（A3）：决策广播 / 定向统一载荷。
// PendingSyncDecisionAsync（广播）/ SyncDecisionAsync（定向）共用本类型；
// TargetWorkItems 空 = 全部在飞。revision-changed → 检查点 + 重对齐 + 租约重验；
// work-item-rescope → 定向重取 Scope 租约（后到拒绝 → Failed）；wrap-up → 旁路注入收束。
namespace Arc.Agent.Harness;
using Arc.Collections;

/// <summary>
/// 子代理决策（广播 / 定向统一载荷）：kind 为 "revision-changed" | "work-item-rescope" |
/// "wrap-up"（"cancel" 走 <c>AIParallelCoordinator.CancelPendingAsync</c>，A2 收束路径）。
/// </summary>
public class AISubAgentDecision {
    /// <summary>"revision-changed" | "work-item-rescope" | "wrap-up"。</summary>
    public string Kind;

    /// <summary>目标 AIRfc Id。</summary>
    public string RfcId;

    /// <summary>决策携带的目标 Revision（revision-changed 为新版本号）。</summary>
    public int RfcRevision;

    /// <summary>定向目标工作项 Id；空 = 全部（广播）。</summary>
    public List<string> TargetWorkItems;

    /// <summary>决策原因（纠偏理由 / 预算压力说明；入决策轨迹审计）。</summary>
    public string Reason;

    public AISubAgentDecision() {
        this.Kind = "";
        this.RfcId = "";
        this.RfcRevision = 0;
        this.TargetWorkItems = new List<string>();
        this.Reason = "";
    }
}
