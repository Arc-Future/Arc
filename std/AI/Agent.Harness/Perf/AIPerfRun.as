// Arc.Agent.Harness.AIPerfRun — AIPerfMonitor 单次运行结果（RFC 043 P1）。
namespace Arc.Agent.Harness;
using Arc.Collections;
using Arc.Diagnostics;

/// <summary>
/// AIPerfMonitor 单次运行结果：进程结果 + 墙钟 + 信号列表 + 超时/崩溃标记 + 异常分类。
/// 纯性能观测面，不改变门判定。
/// </summary>
public class AIPerfRun {
    public ProcessRunResult? Result;
    public long ElapsedMs;
    public List<AIPerfSignal> Signals;
    public bool TimedOut;
    public bool Crashed;
    public AIPerfAnomaly Anomaly;
    public bool SpawnFailed;
    public string SpawnError;
    public string LogPath;

    public AIPerfRun() {
        this.Result = null;
        this.ElapsedMs = 0;
        this.Signals = new List<AIPerfSignal>();
        this.TimedOut = false;
        this.Crashed = false;
        this.Anomaly = AIPerfAnomaly.None;
        this.SpawnFailed = false;
        this.SpawnError = "";
        this.LogPath = "";
    }
}
