// RFC 038 §13（冲突织物）：三 Kind 一表。AIRfc（RfcSpec）/ AIPlan（Plan）/ AITool（ToolPath）
// 共用同一 AICoordinator 登记表与 Acquire / Release / 冲突语义，禁止各搞各的锁。
namespace Arc.Agent;

/// <summary>
/// 冲突织物租约种类（RFC 038 §13 / 043 conflict-fabric §3）。
/// 三 Kind 键空间必须齐——「只锁文件」冒充完成即违反本契约。
/// </summary>
public enum AILeaseKind {
    /// <summary>AITool 副作用路径（现有路径写协调升维；键经 AIWorkspace.ResolvePath 规范化）。</summary>
    ToolPath,
    /// <summary>AIPlan 修订 / 步进（AIPlanGate 突变前获取；键 = "plan:" + PlanId）。</summary>
    Plan,
    /// <summary>AIRfc Spec / 工作项写（AIRfcRuntime 升版前获取；键 = "airfc:" + RfcId）。</summary>
    RfcSpec,
}
