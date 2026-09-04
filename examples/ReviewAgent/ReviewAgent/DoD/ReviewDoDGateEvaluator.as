// 领域二（ReviewAgent）：DoD 判定 — 实现基座 IAIDoDGateEvaluator；
// 用文档审查领域信号判 D0/D3 等价门；未接线门诚实 Pending（禁假绿）。
namespace ReviewAgent.DoD;
using Arc;
using Arc.Agent.Harness;
using ReviewAgent.Tools;

/// <summary>文档审查领域 DoD evaluator：D0 = 文档集完备；D3 = 交叉引用一致；D5/D7 人类门。</summary>
public class ReviewDoDGateEvaluator : IAIDoDGateEvaluator {
    public Task<AIDoDGateResult> EvaluateAsync(
        AIDoDGateKind gate,
        string project,
        AIRfc rfc,
        CancellationToken cancellationToken) {
        string target = project != null && project != "" ? project : ".";
        if (gate == AIDoDGateKind.D0Compile) {
            return this.EvaluateDocSetAsync(target);
        }
        if (gate == AIDoDGateKind.D3Behavior) {
            return this.EvaluateConsistencyAsync(target);
        }
        if (gate == AIDoDGateKind.D5SelfReview) {
            return Task.FromResult(AIDoDGateResult.Human(AIDoDGateKind.D5SelfReview, "self-review proof"));
        }
        if (gate == AIDoDGateKind.D7HumanAccept) {
            return Task.FromResult(AIDoDGateResult.Human(AIDoDGateKind.D7HumanAccept, "collaboration checkpoint"));
        }
        return Task.FromResult(AIDoDGateResult.Pending(gate, "not wired in review domain"));
    }

    /// <summary>D0 等价门：文档集存在且无空文档（可执行、可证伪）。</summary>
    private Task<AIDoDGateResult> EvaluateDocSetAsync(string target) {
        ReviewScanResult scan = ReviewChecks.ScanFolder(target);
        if (scan.Documents.Count == 0) {
            return Task.FromResult(AIDoDGateResult.Fail(
                AIDoDGateKind.D0Compile,
                "doc-set",
                "no markdown documents under '" + target + "'"));
        }
        if (scan.EmptyDocs.Count > 0) {
            return Task.FromResult(AIDoDGateResult.Fail(
                AIDoDGateKind.D0Compile,
                "doc-set",
                scan.Describe()));
        }
        return Task.FromResult(AIDoDGateResult.Pass(
            AIDoDGateKind.D0Compile,
            "doc-set " + scan.Describe()));
    }

    /// <summary>D3 等价门：交叉引用一致性（全部链接目标可解析）。</summary>
    private Task<AIDoDGateResult> EvaluateConsistencyAsync(string target) {
        ReviewScanResult scan = ReviewChecks.ScanFolder(target);
        if (scan.Documents.Count == 0) {
            return Task.FromResult(AIDoDGateResult.Pending(
                AIDoDGateKind.D3Behavior,
                "consistency: no documents"));
        }
        if (scan.BrokenLinks.Count > 0) {
            return Task.FromResult(AIDoDGateResult.Fail(
                AIDoDGateKind.D3Behavior,
                "consistency",
                scan.Describe()));
        }
        return Task.FromResult(AIDoDGateResult.Pass(
            AIDoDGateKind.D3Behavior,
            "consistency " + scan.Describe()));
    }
}
