// Arc.Agent.Harness.AIPerfMonitor — 带超时的捕获运行 + 性能采集（RFC 043 P1）。
//
// 纯 additive 观测面：经 Process.RunCaptureAsync 运行进程，Stopwatch 计墙钟，
// rt_proc_get_stats 采集 CPU/峰值内存，AISignalLog 落盘信号日志；异常分类只进
// AIPerfRun/Signals，不改变任何门判定（判定仍由调用方基于退出码 + 既有信号）。
namespace Arc.Agent.Harness;
using Arc;
using Arc.Collections;
using Arc.Diagnostics;

/// <summary>
/// 带超时的捕获运行与性能采集。RunAsync 返回 <see cref="AIPerfRun"/>（结果/墙钟/信号/
/// 超时/崩溃/异常分类/日志路径）。编译器路径：<c>ARC_COMPILER</c> 优先，否则 PATH 上的 <c>arc</c>。
/// 静态成员承载：Arc 编译器当前限制 <c>static class</c> 不支持静态字段，以普通类 + private 构造承载
/// （对齐 AppContext / DependencyPropertyRegistry 先例）；用户面仍为纯静态访问。
/// </summary>
public class AIPerfMonitor {
    private const long DefaultTimeoutMs = 300000;
    // NTSTATUS 崩溃码。静态 readonly 初始化须走方法调用（编译器 rt_lazy 静态初始化器
    // 仅支持方法调用，算术表达式初始化会读到 0）；纯算术避免 long.Parse 内建拦截缺失。
    private static readonly long NtStatusStackOverflow = AIPerfMonitor.MakeStackOverflow(); // 0xC00000FD
    private static readonly long NtStatusNoMemory = AIPerfMonitor.MakeNoMemory();         // 0xC0000017
    private static readonly long NtStatusCrashBase = AIPerfMonitor.MakeCrashBase();       // 0xC0000000
    private static readonly long Uint32Mask = AIPerfMonitor.MakeUint32Mask();             // 0xFFFFFFFF

    /// <summary>防止实例化——所有成员均为 static。</summary>
    private AIPerfMonitor() {
    }

    /// <summary>默认运行超时（毫秒）。</summary>
    public static long DefaultTimeout {
        get { return AIPerfMonitor.DefaultTimeoutMs; }
    }

    /// <summary>按 arc CLI args 捕获运行（默认超时）。</summary>
    public static async Task<AIPerfRun> RunAsync(string args, string project, CancellationToken cancellationToken) {
        return await AIPerfMonitor.RunAsync(args, project, cancellationToken, AIPerfMonitor.DefaultTimeoutMs);
    }

    /// <summary>按 arc CLI args 捕获运行（显式超时）。</summary>
    public static async Task<AIPerfRun> RunAsync(string args, string project, CancellationToken cancellationToken, long timeoutMs) {
        return await AIPerfMonitor.RunAsync(
            AIPerfMonitor.BuildStartInfo(args), project, cancellationToken, timeoutMs, AIPerfMonitor.ToolName(args));
    }

    /// <summary>按显式 ProcessStartInfo 捕获运行（超时版；供自定义可执行文件/测试夹具使用）。</summary>
    public static async Task<AIPerfRun> RunAsync(ProcessStartInfo startInfo, string project, CancellationToken cancellationToken, long timeoutMs) {
        return await AIPerfMonitor.RunAsync(startInfo, project, cancellationToken, timeoutMs, "run");
    }

    private static async Task<AIPerfRun> RunAsync(
        ProcessStartInfo startInfo, string project, CancellationToken cancellationToken, long timeoutMs, string tool) {
        cancellationToken.ThrowIfCancellationRequested();
        AIPerfRun run = new AIPerfRun();
        AISignalLog log = new AISignalLog(project);
        log.Add(AISignalLevel.Info, "AIPerfMonitor", "perf", AIPerfMonitor.Describe(startInfo), "perf:start");

        Stopwatch sw = Stopwatch.StartNew();
        ProcessRunResult? pr = null;
        string spawnError = "";
        try {
            pr = await Process.RunCaptureAsync(startInfo, (int)timeoutMs, cancellationToken);
        } catch (Exception ex) {
            spawnError = ex != null && ex.Message != null ? ex.Message : "spawn failed";
        }
        sw.Stop();
        run.ElapsedMs = sw.ElapsedMilliseconds;

        if (spawnError != "" || pr == null) {
            run.SpawnFailed = true;
            run.SpawnError = spawnError != "" ? spawnError : "spawn failed";
            run.Anomaly = AIPerfAnomaly.SpawnFailed;
            log.Add(AISignalLevel.Error, "AIPerfMonitor", "perf", run.SpawnError, "perf:spawn_failed");
        } else {
            run.Result = pr;
            run.TimedOut = pr.TimedOut;
            run.Anomaly = AIPerfMonitor.Classify(pr);
            run.Crashed = run.Anomaly == AIPerfAnomaly.Crash
                || run.Anomaly == AIPerfAnomaly.Oom
                || run.Anomaly == AIPerfAnomaly.StackOverflow;
            log.Add(AISignalLevel.Info, "AIPerfMonitor", "perf", "wall=" + run.ElapsedMs + "ms", "perf:wall");
            ProcessRunStats? stats = pr.Stats;
            if (stats != null) {
                log.Add(AISignalLevel.Info, "AIPerfMonitor", "perf", "peak_mem=" + stats.PeakMemoryBytes + "B", "perf:peak_memory");
                log.Add(AISignalLevel.Info, "AIPerfMonitor", "perf", "cpu_user=" + stats.CpuUserMs + "ms", "perf:cpu_user");
                log.Add(AISignalLevel.Info, "AIPerfMonitor", "perf", "cpu_kernel=" + stats.CpuKernelMs + "ms", "perf:cpu_kernel");
            }
            log.Add(AISignalLevel.Info, "AIPerfMonitor", "perf", "exit=" + pr.ExitCode, "perf:exit");
            if (run.TimedOut) {
                log.Add(AISignalLevel.Warn, "AIPerfMonitor", "perf",
                    "timed out after " + run.ElapsedMs + "ms (limit " + timeoutMs + "ms)", "perf:timedout");
            }
            if (run.Anomaly != AIPerfAnomaly.None) {
                log.Add(AISignalLevel.Warn, "AIPerfMonitor", "perf",
                    AIPerfMonitor.AnomalyName(run.Anomaly), "perf:anomaly=" + AIPerfMonitor.AnomalyName(run.Anomaly));
            }
        }
        string logPath = await log.WriteAsync(tool, cancellationToken);
        run.LogPath = logPath;
        run.Signals = log.Signals;
        return run;
    }

    /// <summary>
    /// 退出分类（跨平台）：SpawnFailed（null 结果）→ 超时（TimedOut）→ 信号终止/崩溃 NTSTATUS →
    /// Oom/StackOverflow 细分 → 其余非零退出归 None（正常失败非异常）。Windows 崩溃码经 32 位无符号重释比较。
    /// </summary>
    public static AIPerfAnomaly Classify(ProcessRunResult? r) {
        if (r == null) {
            return AIPerfAnomaly.SpawnFailed;
        }
        if (r.TimedOut) {
            return AIPerfAnomaly.Timeout;
        }
        if (r.ExitCode != 0) {
            ProcessRunStats? stats = r.Stats;
            if (stats != null && stats.ExitReason == ProcessExitReason.SignalTerminated) {
                return AIPerfAnomaly.Crash;
            }
            long uexit = (long)r.ExitCode & AIPerfMonitor.Uint32Mask;
            if (uexit == AIPerfMonitor.NtStatusStackOverflow) {
                return AIPerfAnomaly.StackOverflow;
            }
            if (uexit == AIPerfMonitor.NtStatusNoMemory) {
                return AIPerfAnomaly.Oom;
            }
            if (uexit >= AIPerfMonitor.NtStatusCrashBase) {
                return AIPerfAnomaly.Crash;
            }
        }
        return AIPerfAnomaly.None;
    }

    /// <summary>
    /// 自适应折叠：把日志路径指针附到门 Detail 尾部（供 deep-dive 定位）；
    /// wall/峰值内存等性能指标留在 PerfSignals（日志消费面），不进 Detail 防噪声。
    /// 门判定不改变。
    /// </summary>
    public static string AttachLogPointer(string detail, AIPerfRun perf) {
        string logPath = perf.LogPath;
        if (logPath == null || logPath == "") {
            return detail != null ? detail : "";
        }
        string baseDetail = detail != null ? detail : "";
        string pointer = "perf: log=" + logPath;
        if (baseDetail != "") {
            return baseDetail + "\n" + pointer;
        }
        return pointer;
    }

    /// <summary>构造 arc CLI 启动信息：ARC_COMPILER 优先（对齐 QualityCli），否则 PATH 上的 arc。</summary>
    private static ProcessStartInfo BuildStartInfo(string args) {
        ProcessStartInfo si = new ProcessStartInfo();
        string compiler = Environment.GetEnvironmentVariable("ARC_COMPILER");
        if (compiler != null && compiler != "") {
            si.FileName = compiler;
            si.Arguments = args;
        } else if (Environment.IsWindows()) {
            si.FileName = "cmd.exe";
            si.Arguments = "/c arc " + args;
        } else {
            si.FileName = "/bin/sh";
            si.Arguments = "-c arc " + args;
        }
        return si;
    }

    private static long MakeStackOverflow() { return ((long)49152) * 65536 + 253; }
    private static long MakeNoMemory() { return ((long)49152) * 65536 + 23; }
    private static long MakeCrashBase() { return ((long)49152) * 65536; }
    private static long MakeUint32Mask() { return ((long)65535) * 65536 + 65535; }

    /// <summary>日志 tool 名 = args 首个 token（build/test/inspect…）；空则 "run"。</summary>
    private static string ToolName(string args) {
        string a = args != null ? args.Trim() : "";
        int sp = a.IndexOf(" ");
        if (sp < 0) {
            return a != "" ? a : "run";
        }
        return a.Substring(0, sp);
    }

    private static string Describe(ProcessStartInfo si) {
        if (si == null) {
            return "";
        }
        string s = si.FileName;
        if (si.Arguments != null && si.Arguments != "") {
            s = s + " " + si.Arguments;
        }
        return s;
    }

    private static string AnomalyName(AIPerfAnomaly anomaly) {
        if (anomaly == AIPerfAnomaly.Crash) { return "crash"; }
        if (anomaly == AIPerfAnomaly.Oom) { return "oom"; }
        if (anomaly == AIPerfAnomaly.StackOverflow) { return "stack-overflow"; }
        if (anomaly == AIPerfAnomaly.Timeout) { return "timeout"; }
        if (anomaly == AIPerfAnomaly.MemorySpike) { return "memory-spike"; }
        if (anomaly == AIPerfAnomaly.SlowCompile) { return "slow-compile"; }
        if (anomaly == AIPerfAnomaly.SpawnFailed) { return "spawn-failed"; }
        return "none";
    }
}
