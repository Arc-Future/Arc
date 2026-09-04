// 方案 B（conflict-branch §5/§8）：冲突记录 — 机器检测的 L2 Spec 矛盾登记，等待人 CCB 裁决。
// Status 为 wire 串（Open | Resolved | Escalated | Rejected），与 RFC 契约一致。
// 归属 Arc.Agent.Harness；裁决唯一入口 = 人（AIConflictResolver，禁自动选胜者）。
namespace Arc.Agent.Harness;
using Arc.Collections;

/// <summary>冲突种类（三级统一仲裁：L1 资源租约 / L2 Spec 矛盾 / L3 git 合并）。</summary>
public enum AIConflictKind {
    /// <summary>L1：资源租约冲突（ToolPath / Plan / RfcSpec 后到拒绝；冲突织物已落地）。</summary>
    LeaseConflict,
    /// <summary>L2：Spec 逻辑矛盾（同 acceptance 项被反方向覆盖，A.1 顺序矛盾判定）。</summary>
    SpecContradiction,
    /// <summary>L3：git 合并冲突（同文件双改；方案 B B3 落地面）。</summary>
    MergeConflict,
}

/// <summary>
/// 冲突记录（机器检测 → 人 CCB 裁决的唯一载体）。Status 常量见
/// <see cref="StatusOpen"/> 等；<see cref="AIConflictResolver.ResolveAsync"/> 裁决后写新
/// Revision 基线，<see cref="AIConflictResolver.RejectAsync"/> 拒绝后记录 Rejected 且 AIRfc
/// 转 Rejected。Before/After Acceptance 快照供新基线重建（维护冲突前方向 / 采纳被拦截方向）。
/// </summary>
public class AIConflictRecord {
    public string ConflictId;
    public AIConflictKind Kind;
    /// <summary>冲突承载的 AIRfc（新基线 / 拒绝联动目标）。</summary>
    public string RfcId;
    /// <summary>冲突发生时 AIRfc Revision（前置 Active 版号）。</summary>
    public int Revision;
    /// <summary>冲突面（acceptance 项 / 路径 / 冲突文件）。</summary>
    public List<string> Resources;
    /// <summary>冲突双方（会话 / 分支 / 来源）。</summary>
    public List<string> Parties;
    /// <summary>diff / 摘要（机器检测证据）。</summary>
    public string Evidence;
    /// <summary>Open | Resolved | Escalated | Rejected。</summary>
    public string Status;
    /// <summary>CCB 裁决人（ResolveAsync 的 resolvedBy；空 = 尚未人裁，禁自动选胜者）。</summary>
    public string ResolvedBy;
    /// <summary>CCB 裁决决定（accept-before / accept-after / 自由文本）。</summary>
    public string Decision;
    /// <summary>冲突时 Active 版 Acceptance 快照（裁决维持方向时作新基线）。</summary>
    public AIAcceptanceSpec BeforeAcceptance;
    /// <summary>被拦截的反向版 Acceptance 快照（裁决采纳时作新基线）。</summary>
    public AIAcceptanceSpec AfterAcceptance;

    public AIConflictRecord() {
        this.ConflictId = "";
        this.Kind = AIConflictKind.SpecContradiction;
        this.RfcId = "";
        this.Revision = 0;
        this.Resources = new List<string>();
        this.Parties = new List<string>();
        this.Evidence = "";
        this.Status = AIConflictRecord.StatusOpen();
        this.ResolvedBy = "";
        this.Decision = "";
        this.BeforeAcceptance = new AIAcceptanceSpec();
        this.AfterAcceptance = new AIAcceptanceSpec();
    }

    /// <summary>是否 Open（待裁决）。</summary>
    public bool IsOpen {
        get { return this.Status == AIConflictRecord.StatusOpen(); }
    }

    public static string StatusOpen() { return "Open"; }
    public static string StatusResolved() { return "Resolved"; }
    public static string StatusEscalated() { return "Escalated"; }
    public static string StatusRejected() { return "Rejected"; }
}
