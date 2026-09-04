// Arc.Diagnostics.ProcessRunResult — RunCapture 返回容器。

namespace Arc.Diagnostics;

/// <summary>
/// RunCapture 返回容器——退出码、stdout、stderr、超时标记与可选资源统计。
/// <see cref="TimedOut"/> / <see cref="Stats"/> 为 RFC 043 P1 新增（additive）。
/// </summary>
public class ProcessRunResult {
    public int ExitCode { get; set; }
    public string StandardOutput { get; set; }
    public string StandardError { get; set; }
    public bool TimedOut { get; set; }
    public ProcessRunStats? Stats { get; set; }

    public ProcessRunResult() {
        ExitCode = -1;
        StandardOutput = "";
        StandardError = "";
        TimedOut = false;
        Stats = null;
    }
}
