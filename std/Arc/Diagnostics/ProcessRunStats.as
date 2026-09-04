// Arc.Diagnostics.ProcessRunStats — 子进程资源统计（rt_proc_get_stats 契约消费）。
//
// RFC 043 P1 性能采集：捕获运行附加进程资源统计，供 Coding Harness 门 Detail /
// AIPerfMonitor 异常分类使用。additive：不改变既有 Process 调用语义。

namespace Arc.Diagnostics;

/// <summary>进程退出形态。</summary>
public enum ProcessExitReason {
    /// <summary>尚未退出（统计采样时仍在运行）。</summary>
    NotExited,
    /// <summary>正常退出（Windows 无信号语义的崩溃码仍在 <see cref="ProcessRunResult.ExitCode"/> 暴露）。</summary>
    NormalExit,
    /// <summary>被信号终止（POSIX WIFSIGNALED；信号号见 <see cref="ProcessRunStats.ExitSignal"/>）。</summary>
    SignalTerminated
}

/// <summary>
/// 子进程资源统计——墙钟、CPU（用户/内核）、峰值内存与退出形态。
/// <c>PeakMemoryBytes</c>：Windows = PeakWorkingSetSize；POSIX = ru_maxrss 归一为字节。
/// </summary>
public class ProcessRunStats {
    public long ElapsedMs { get; set; }
    public long PeakMemoryBytes { get; set; }
    public long CpuUserMs { get; set; }
    public long CpuKernelMs { get; set; }
    public ProcessExitReason ExitReason { get; set; }
    public int ExitSignal { get; set; }

    public ProcessRunStats() {
        ElapsedMs = 0;
        PeakMemoryBytes = 0;
        CpuUserMs = 0;
        CpuKernelMs = 0;
        ExitReason = ProcessExitReason.NotExited;
        ExitSignal = 0;
    }

    /// <summary>把 rt_proc_get_stats 的原始 exit_reason 归一到枚举：0=正常；&gt;0=信号终止（含信号号）；&lt;0=未退出。</summary>
    public static ProcessExitReason ClassifyExitReason(int raw) {
        if (raw == 0) {
            return ProcessExitReason.NormalExit;
        }
        if (raw > 0) {
            return ProcessExitReason.SignalTerminated;
        }
        return ProcessExitReason.NotExited;
    }
}
