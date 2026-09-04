// Arc.Agent.Harness.AIPerfAnomaly — 性能采集异常分类（RFC 043 P1）。
namespace Arc.Agent.Harness;

/// <summary>
/// AIPerfMonitor 运行异常分类。仅用于性能观测与信号日志，不改变门判定。
/// </summary>
public enum AIPerfAnomaly {
    /// <summary>无异常。</summary>
    None,
    /// <summary>进程崩溃（Windows 崩溃 NTSTATUS 如 0xC0000005，或 POSIX 信号终止）。</summary>
    Crash,
    /// <summary>内存耗尽（Windows STATUS_NO_MEMORY 0xC0000017）。</summary>
    Oom,
    /// <summary>栈溢出（Windows STATUS_STACK_OVERFLOW 0xC00000FD）。</summary>
    StackOverflow,
    /// <summary>超时被 Kill（WaitForExit 超时）。</summary>
    Timeout,
    /// <summary>峰值内存超阈值（memorySpikeBytes 启用时）。</summary>
    MemorySpike,
    /// <summary>编译/运行慢于阈值（slowCompileMs 启用时）。</summary>
    SlowCompile,
    /// <summary>进程启动失败（spawn 异常 / 句柄无效）。</summary>
    SpawnFailed
}
