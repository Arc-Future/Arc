// 方案 B（conflict-branch §2/§5）：L2 Spec 矛盾检测 — 字段级结构化 diff（AIAcceptanceSpec.Items
// 条目级比对）。同 acceptance 项被不同来源（AIRfc.Source）反方向覆盖 → 返回冲突证据；同来源
// 修订（多轮讨论 refine，场景 1.3）不判矛盾。纯函数，机器只检测不裁决。
namespace Arc.Agent.Harness;
using Arc.Collections;
using Arc.Text;

/// <summary>L2 检测结果：冲突面 + 证据 + 双方 Acceptance 快照（供 AIConflictRecord 与新基线）。</summary>
public class AISpecConflict {
    public string RfcId;
    public int Revision;
    /// <summary>冲突前来源（当前 Active 版 owner）。</summary>
    public string SourceA;
    /// <summary>发起反方向覆盖的来源（被拦截方）。</summary>
    public string SourceB;
    /// <summary>冲突面（"<rfcId>@acceptance[i]" 等）。</summary>
    public List<string> Resources;
    /// <summary>diff 摘要（旧版 → 新版条目逐行）。</summary>
    public string Evidence;
    public AIAcceptanceSpec BeforeAcceptance;
    public AIAcceptanceSpec AfterAcceptance;

    public AISpecConflict() {
        this.RfcId = "";
        this.Revision = 0;
        this.SourceA = "";
        this.SourceB = "";
        this.Resources = new List<string>();
        this.Evidence = "";
        this.BeforeAcceptance = new AIAcceptanceSpec();
        this.AfterAcceptance = new AIAcceptanceSpec();
    }
}

/// <summary>
/// L2 Spec 矛盾检测器（纯函数；机器只检测，不裁决）。冲突面限定结构化 acceptance 条目
/// （A.1 口径：`AIAcceptanceSpec.Items` 条目级可比）；Intention/Design 文本面无机器可比结构。
/// </summary>
public static class AISpecConflictDetector {
    /// <summary>
    /// 判定一次升版是否构成 Spec 矛盾：旧版结构化 acceptance 项（按索引）被不同来源的新版
    /// 覆盖（字段级内容变化）→ 反方向覆盖信号 → 返回冲突；同来源 / 非结构化面 / 无覆盖 →
    /// null。
    /// </summary>
    public static AISpecConflict? Detect(
        AIRfc current,
        AIAcceptanceSpec? nextAcceptance,
        string source,
        string currentSource) {
        if (current == null || current.Acceptance == null) {
            return null;
        }
        string src = source != null ? source : "";
        string curSrc = currentSource != null ? currentSource : "";
        // 同来源修订 = 多轮讨论 refine（场景 1.3），不判矛盾。
        if (src == curSrc) {
            return null;
        }
        if (nextAcceptance == null || !current.Acceptance.HasStructuredItems || !nextAcceptance.HasStructuredItems) {
            return null;
        }
        List<string> resources = new List<string>();
        StringBuilder evidence = new StringBuilder();
        bool found = false;
        int i = 0;
        int n = current.Acceptance.Items.Count;
        while (i < n) {
            if (i < nextAcceptance.Items.Count) {
                AIAcceptanceItem oldItem = current.Acceptance.Items[i];
                AIAcceptanceItem newItem = nextAcceptance.Items[i];
                if (oldItem != null && newItem != null && !oldItem.IsEmpty && !AISpecConflictDetector.SameItem(oldItem, newItem)) {
                    resources.Add(current.RfcId + "@acceptance[" + i + "]");
                    evidence.Append("acceptance[" + i + "] '" + oldItem.ToLine() + "' → '" + newItem.ToLine() + "'\n");
                    found = true;
                }
            }
            i = i + 1;
        }
        if (!found) {
            return null;
        }
        AISpecConflict c = new AISpecConflict();
        c.RfcId = current.RfcId;
        c.Revision = current.Revision;
        c.SourceA = curSrc;
        c.SourceB = src;
        c.Resources = resources;
        c.Evidence = evidence.ToString();
        c.BeforeAcceptance = current.Acceptance;
        c.AfterAcceptance = nextAcceptance;
        return c;
    }

    private static bool SameItem(AIAcceptanceItem a, AIAcceptanceItem b) {
        if (a == null || b == null) {
            return false;
        }
        return a.Scenario == b.Scenario
            && a.Assertions == b.Assertions
            && a.TestName == b.TestName
            && a.VerifyCommand == b.VerifyCommand;
    }
}
