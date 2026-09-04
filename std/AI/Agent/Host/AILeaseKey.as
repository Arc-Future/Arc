// RFC 038 §13 / 043 conflict-fabric §3：统一租约键 = Kind + Norm(Id)。
// ToolPath 经 AIWorkspace.ResolvePath 规范化；Plan = "plan:"+PlanId；RfcSpec = "airfc:"+RfcId。
namespace Arc.Agent;

/// <summary>
/// 统一租约键 = <see cref="AILeaseKind"/> + 规范化资源标识（RFC 043 conflict-fabric §3）。
/// ToolPath 资源 id 为原始路径（Acquire 时经 workspace 解析为规范键）；Plan / RfcSpec
/// 分别带 "plan:" / "airfc:" 前缀以分隔键空间。同 Kind 同资源 = 同一租约（冲突仲裁按此键）。
/// </summary>
public class AILeaseKey {
    /// <summary>租约种类。</summary>
    public AILeaseKind Kind;
    /// <summary>规范化资源标识（ToolPath 原路径 / "plan:"+PlanId / "airfc:"+RfcId）。</summary>
    public string ResourceId;

    public AILeaseKey(AILeaseKind kind, string resourceId) {
        this.Kind = kind;
        this.ResourceId = resourceId != null ? resourceId : "";
    }

    /// <summary>ToolPath 键（path 为原始路径；Acquire 时经 workspace 规范化）。</summary>
    public static AILeaseKey ToolPath(string path) {
        return new AILeaseKey(AILeaseKind.ToolPath, path != null ? path : "");
    }

    /// <summary>Plan 键：ResourceId = "plan:" + planId。</summary>
    public static AILeaseKey Plan(string planId) {
        string id = planId != null ? planId : "";
        return new AILeaseKey(AILeaseKind.Plan, "plan:" + id);
    }

    /// <summary>RfcSpec 键：ResourceId = "airfc:" + rfcId。</summary>
    public static AILeaseKey RfcSpec(string rfcId) {
        string id = rfcId != null ? rfcId : "";
        return new AILeaseKey(AILeaseKind.RfcSpec, "airfc:" + id);
    }
}
