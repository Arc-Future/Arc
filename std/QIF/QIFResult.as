namespace Arc.QIF;

using Arc.Collections;

/// <summary>
/// 单个测试的执行结果。对标 XUnit TestResult。
/// </summary>
internal class QIFResult {
    public string Name;
    public QIFTestKind Kind;
    public QIFTestStatus Status;
    public long DurationNs;
    public string ErrorMessage;
    public string SkipReason;
    public string StackTrace;
    public string Output;
    public List<string> Traits;

    public QIFResult(string name, QIFTestKind kind, QIFTestStatus status, long durationNs) {
        this.Name = name;
        this.Kind = kind;
        this.Status = status;
        this.DurationNs = durationNs;
        this.ErrorMessage = "";
        this.SkipReason = "";
        this.StackTrace = "";
        this.Output = "";
        this.Traits = new List<string>();
    }

    public QIFResult(string name, QIFTestKind kind, QIFTestStatus status, long durationNs, string errorMessage) {
        this.Name = name;
        this.Kind = kind;
        this.Status = status;
        this.DurationNs = durationNs;
        this.ErrorMessage = errorMessage;
        this.SkipReason = "";
        this.StackTrace = "";
        this.Output = "";
        this.Traits = new List<string>();
    }

    public bool IsPassed { get { return this.Status == QIFTestStatus.Pass; } }
    public bool IsFailed { get { return this.Status == QIFTestStatus.Fail; } }
    public bool IsSkipped { get { return this.Status == QIFTestStatus.Skip; } }
    public bool IsError { get { return this.Status == QIFTestStatus.Error; } }

    public string DurationMs {
        get {
            long ms = this.DurationNs / 1000000;
            if (ms < 1) { return "<1ms"; }
            return ms.ToString() + "ms";
        }
    }
}
