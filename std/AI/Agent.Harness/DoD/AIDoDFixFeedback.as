// RFC 043 场景 2.3/4.3：结构化失败回喂 — D0–D3 判定失败时把「门 + 信号 + 诊断明细」折叠为
// 模型可消费文本（D0 编译诊断 / D3 --logger json 明细均落在 AIDoDGateResult.Detail）。
namespace Arc.Agent.Harness;
using Arc;
using Arc.Collections;

/// <summary>
/// 一轮失败的结构化回喂：失败门清单（含诊断文本 / 退出码 / --logger json 明细）+ 轮次号。
/// 由 <see cref="AIDoDOrchestrator.RunFixLoopAsync"/> 在每轮失败时构建，传给
/// <see cref="IAIFixRoundProvider.FixAsync"/> 驱动修复。
/// </summary>
public class AIDoDFixFeedback {
    public string Project;
    public int RoundNumber;
    public List<AIDoDGateResult> FailingGates;

    public AIDoDFixFeedback() {
        this.Project = "";
        this.RoundNumber = 0;
        this.FailingGates = new List<AIDoDGateResult>();
    }

    /// <summary>从一轮门结果构建反馈：仅收集 Failed / NeedsHuman 门（Pending 属数据不足，不驱动修复）。</summary>
    public static AIDoDFixFeedback FromFailing(
        List<AIDoDGateResult> results,
        int roundNumber,
        string project) {
        AIDoDFixFeedback fb = new AIDoDFixFeedback();
        fb.Project = project != null ? project : "";
        fb.RoundNumber = roundNumber;
        if (results != null) {
            int i = 0;
            int n = results.Count;
            while (i < n) {
                AIDoDGateResult r = results[i];
                if (r != null
                    && (r.Status == AIDoDGateStatus.Failed || r.Status == AIDoDGateStatus.NeedsHuman)) {
                    fb.FailingGates.Add(r);
                }
                i = i + 1;
            }
        }
        return fb;
    }

    /// <summary>折叠为模型可消费回喂文本（门名 + 信号 + 诊断明细）。</summary>
    public string Describe() {
        string s = "DoD verification failed (round " + this.RoundNumber + ")"
            + (this.Project != "" ? " in project " + this.Project : "");
        if (this.FailingGates == null || this.FailingGates.Count == 0) {
            return s + " — no failing gate detail";
        }
        int i = 0;
        int n = this.FailingGates.Count;
        while (i < n) {
            AIDoDGateResult r = this.FailingGates[i];
            s = s + "\n\n[" + AIDoDFixFeedback.GateName(r.Gate) + " " + this.StatusName(r.Status) + "] " + r.Signal;
            // 结构化诊断优先：ErrorItems（如 D0 编译错误）逐条折叠单行，LLM 直接消费；否则回退 Detail。
            if (r.ErrorItems != null && r.ErrorItems.Count > 0) {
                int j = 0;
                int m = r.ErrorItems.Count;
                while (j < m) {
                    s = s + "\n  - " + r.ErrorItems[j].Format();
                    j = j + 1;
                }
            } else if (r.Detail != null && r.Detail != "") {
                s = s + "\n" + r.Detail;
            }
            i = i + 1;
        }
        return s;
    }

    private static string GateName(AIDoDGateKind gate) {
        if (gate == AIDoDGateKind.D0Compile) { return "D0 compile"; }
        if (gate == AIDoDGateKind.D1Semantics) { return "D1 semantics"; }
        if (gate == AIDoDGateKind.D2Contract) { return "D2 contract"; }
        if (gate == AIDoDGateKind.D3Behavior) { return "D3 behavior"; }
        if (gate == AIDoDGateKind.D4DiffCoverage) { return "D4 diff coverage"; }
        if (gate == AIDoDGateKind.D5SelfReview) { return "D5 self-review"; }
        if (gate == AIDoDGateKind.D6AntiPattern) { return "D6 anti-pattern"; }
        return "D7 human accept";
    }

    private string StatusName(AIDoDGateStatus status) {
        if (status == AIDoDGateStatus.Passed) { return "Passed"; }
        if (status == AIDoDGateStatus.Failed) { return "Failed"; }
        if (status == AIDoDGateStatus.NeedsHuman) { return "NeedsHuman"; }
        return "Pending";
    }
}
