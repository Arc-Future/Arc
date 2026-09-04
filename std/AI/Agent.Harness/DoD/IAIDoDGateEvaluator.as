// RFC 043：领域 DoD 判定注入点 — 基座只定义契约；领域实现，禁基座写死领域信号源。
namespace Arc.Agent.Harness;
using Arc;

/// <summary>领域判定注入点；领域包实现，基座不写死领域信号源。</summary>
public interface IAIDoDGateEvaluator {
    Task<AIDoDGateResult> EvaluateAsync(
        AIDoDGateKind gate,
        string project,
        AIRfc rfc,
        CancellationToken cancellationToken);
}
