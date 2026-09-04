// RFC 043：D0–D7 可执行 DoD 门种类与单门结果。
namespace Arc.Agent.Harness;
using Arc.Collections;

/// <summary>可执行完成定义门（D0–D7 + D9 性能门）。</summary>
public enum AIDoDGateKind {
    D0Compile,
    D1Semantics,
    D2Contract,
    D3Behavior,
    D4DiffCoverage,
    D5SelfReview,
    D6AntiPattern,
    D7HumanAccept,
    D9Perf
}

/// <summary>单门判定结果。</summary>
public enum AIDoDGateStatus {
    Pending,
    Passed,
    Failed,
    NeedsHuman
}

/// <summary>单门运行记录。</summary>
public class AIDoDGateResult {
    public AIDoDGateKind Gate;
    public AIDoDGateStatus Status;
    public string Signal;
    public string Detail;
    public List<AIPerfSignal> PerfSignals;
    public List<AIDoDErrorItem> ErrorItems;

    public AIDoDGateResult() {
        this.Gate = AIDoDGateKind.D0Compile;
        this.Status = AIDoDGateStatus.Pending;
        this.Signal = "";
        this.Detail = "";
        this.PerfSignals = new List<AIPerfSignal>();
        this.ErrorItems = new List<AIDoDErrorItem>();
    }

    public static AIDoDGateResult Pass(AIDoDGateKind gate, string signal) {
        AIDoDGateResult r = new AIDoDGateResult();
        r.Gate = gate;
        r.Status = AIDoDGateStatus.Passed;
        r.Signal = signal != null ? signal : "";
        r.Detail = "";
        return r;
    }

    public static AIDoDGateResult Fail(AIDoDGateKind gate, string signal, string detail) {
        AIDoDGateResult r = new AIDoDGateResult();
        r.Gate = gate;
        r.Status = AIDoDGateStatus.Failed;
        r.Signal = signal != null ? signal : "";
        r.Detail = detail != null ? detail : "";
        return r;
    }

    public static AIDoDGateResult Human(AIDoDGateKind gate, string signal) {
        AIDoDGateResult r = new AIDoDGateResult();
        r.Gate = gate;
        r.Status = AIDoDGateStatus.NeedsHuman;
        r.Signal = signal != null ? signal : "";
        r.Detail = "";
        return r;
    }

    public static AIDoDGateResult Pending(AIDoDGateKind gate, string signal) {
        AIDoDGateResult r = new AIDoDGateResult();
        r.Gate = gate;
        r.Status = AIDoDGateStatus.Pending;
        r.Signal = signal != null ? signal : "";
        r.Detail = "";
        return r;
    }

    public bool IsBlocking {
        get {
            return this.Status == AIDoDGateStatus.Failed || this.Status == AIDoDGateStatus.NeedsHuman;
        }
    }
}
