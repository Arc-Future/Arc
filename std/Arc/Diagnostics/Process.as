// Arc.Diagnostics.Process — 子进程控制主类。

namespace Arc.Diagnostics;

using Arc.IO;
using Arc.Threading;
using Arc.Collections;
using Arc;          // Task<T>, CancellationToken

/// <summary>
/// 子进程控制——启动/等待/终止子进程，访问其 stdin/stdout/stderr。
/// 底层经 rt_process native 契约（crates/runtime/rt_proc.c）。
/// </summary>
public class Process : IDisposable {
    protected NativePtr _handle;
    protected ProcessStream? _stdinStream;
    protected ProcessStream? _stdoutStream;
    protected ProcessStream? _stderrStream;
    protected int _pid;
    protected bool _started;
    protected bool _disposed;
    protected Thread? _exitWatcher;

    public ProcessStartInfo StartInfo { get; set; }

    public int Id {
        get { return _pid; }
    }

    public int ExitCode {
        get { return rt_process.rt_proc_get_exit_code(_handle); }
    }

    /// <summary>Windows GetExitCodeProcess 对仍在运行的进程返回 STILL_ACTIVE(259)。</summary>
    private const int StillActive = 259;

    public bool HasExited {
        get {
            int code = rt_process.rt_proc_get_exit_code(_handle);
            // 运行中进程 rt_proc_get_exit_code 返回 STILL_ACTIVE(259)；已退出返回真实码。
            // 不能以 `code >= 0` 判断——259>=0 会把运行中进程误判为已退出。
            return code != StillActive;
        }
    }

    public ProcessStream StandardInput {
        get { return _stdinStream; }
    }

    public ProcessStream StandardOutput {
        get { return _stdoutStream; }
    }

    public ProcessStream StandardError {
        get { return _stderrStream; }
    }

    public bool EnableRaisingEvents { get; set; }

    /// <summary>进程退出时触发（需 EnableRaisingEvents = true）。</summary>
    public Action<Process>? Exited { get; set; }

    public Process() {
        _handle = null;
        StartInfo = new ProcessStartInfo();
        _stdinStream = null;
        _stdoutStream = null;
        _stderrStream = null;
        _pid = 0;
        _started = false;
        _disposed = false;
        EnableRaisingEvents = false;
        _exitWatcher = null;
        Exited = null;
    }

    public void Start() {
        if (_started) { throw new InvalidOperationException("Process already started"); }
        if (_disposed) { throw new ObjectDisposedException("Process"); }

        int stdinFd = -1;
        int stdoutFd = -1;
        int stderrFd = -1;
        NativePtr h = rt_process.rt_proc_spawn(
            StartInfo.FileName,
            StartInfo.Arguments,
            StartInfo.WorkingDirectory,
            StartInfo.RedirectStandardInput ? 1 : 0,
            StartInfo.RedirectStandardOutput ? 1 : 0,
            StartInfo.RedirectStandardError ? 1 : 0,
            StartInfo.CreateNoWindow ? 1 : 0,
            out stdinFd,
            out stdoutFd,
            out stderrFd
        );
        if (h == null) {
            throw new InvalidOperationException("Failed to start process: " + StartInfo.FileName);
        }
        _handle = h;
        _pid = rt_process.rt_proc_get_pid(h);
        _started = true;
        if (stdinFd >= 0) { _stdinStream = new ProcessStream(stdinFd, false, true); }
        if (stdoutFd >= 0) { _stdoutStream = new ProcessStream(stdoutFd, true, false); }
        if (stderrFd >= 0) { _stderrStream = new ProcessStream(stderrFd, true, false); }
        if (EnableRaisingEvents) {
            _exitWatcher = new Thread(() => _watchExit());
            _exitWatcher.Start();
        }
    }

    private void _watchExit() {
        rt_process.rt_proc_wait(_handle, -1);
        if (Exited != null) { Exited(this); }
    }

    public bool WaitForExit(int timeoutMs) {
        int r = rt_process.rt_proc_wait(_handle, timeoutMs);
        return r == 0;
    }

    public void WaitForExit() {
        rt_process.rt_proc_wait(_handle, -1);
    }

    public void Kill() {
        if (!_started) { return; }
        rt_process.rt_proc_kill(_handle);
    }

    /// <summary>终止进程。整棵进程树终止暂未支持（rt_proc_kill 仅终止单进程，整树能力待新增 rt ABI）。</summary>
    public void Kill(bool entireProcessTree) {
        if (!_started) { return; }
        rt_process.rt_proc_kill(_handle);
    }

    public void Close() {
        this.Dispose();
    }

    public virtual void Dispose() {
        if (_disposed) { return; }
        _disposed = true;
        if (_exitWatcher != null) { _exitWatcher.Join(); _exitWatcher = null; }
        if (_stdinStream != null) { _stdinStream.Dispose(); }
        if (_stdoutStream != null) { _stdoutStream.Dispose(); }
        if (_stderrStream != null) { _stderrStream.Dispose(); }
        if (_started) {
            rt_process.rt_proc_close(_handle);
        }
    }

    public static Process Start(ProcessStartInfo startInfo) {
        Process p = new Process();
        p.StartInfo = startInfo;
        p.Start();
        return p;
    }

    /// <summary>进程资源统计（rt_proc_get_stats；句柄无效或采集失败 → null）。</summary>
    public ProcessRunStats? GetRunStats() {
        if (!_started || _handle == null) {
            return null;
        }
        long userMs = 0;
        long kernelMs = 0;
        long peakMem = 0;
        int exitReason = -1;
        int ok = rt_process.rt_proc_get_stats(_handle, out userMs, out kernelMs, out peakMem, out exitReason);
        if (ok != 0) {
            return null;
        }
        ProcessRunStats s = new ProcessRunStats();
        s.CpuUserMs = userMs;
        s.CpuKernelMs = kernelMs;
        s.PeakMemoryBytes = peakMem;
        s.ExitReason = ProcessRunStats.ClassifyExitReason(exitReason);
        s.ExitSignal = exitReason > 0 ? exitReason : 0;
        return s;
    }

    public static ProcessRunResult RunCapture(ProcessStartInfo startInfo) {
        return Process.RunCapture(startInfo, -1);
    }

    /// <summary>
    /// 捕获运行（RFC 043 P1 超时版）：<paramref name="timeoutMs"/> 内未退出 → Kill →
    /// <see cref="ProcessRunResult.TimedOut"/> 标记。返回结果附加 <see cref="ProcessRunResult.Stats"/>
    /// （墙钟经 Stopwatch 计时；CPU/峰值内存经 rt_proc_get_stats）。
    /// </summary>
    public static ProcessRunResult RunCapture(ProcessStartInfo startInfo, int timeoutMs) {
        startInfo.RedirectStandardOutput = true;
        startInfo.RedirectStandardError = true;
        Process p = Process.Start(startInfo);
        Stopwatch sw = Stopwatch.StartNew();
        // 并发读 stdout/stderr，避免子进程 stderr 缓冲满时死锁。
        // 用 List<string>（引用类型）累积行——Arc lambda 捕获引用类型可正确修改，
        // 捕获值类型局部变量赋值在闭包外不可见。
        List<string> stdoutLines = new List<string>();
        List<string> stderrLines = new List<string>();
        Thread stdoutThread = new Thread(() => {
            string? line = p.StandardOutput.ReadLine();
            while (line != null) {
                stdoutLines.Add(line);
                line = p.StandardOutput.ReadLine();
            }
        });
        Thread stderrThread = new Thread(() => {
            string? line = p.StandardError.ReadLine();
            while (line != null) {
                stderrLines.Add(line);
                line = p.StandardError.ReadLine();
            }
        });
        stdoutThread.Start();
        stderrThread.Start();
        bool exited = p.WaitForExit(timeoutMs);
        if (!exited) {
            p.Kill();
            p.WaitForExit();
        }
        stdoutThread.Join();
        stderrThread.Join();
        sw.Stop();
        string stdoutText = "";
        int i = 0;
        while (i < stdoutLines.Count) {
            if (stdoutText.Length > 0) { stdoutText = stdoutText + "\n"; }
            stdoutText = stdoutText + stdoutLines[i];
            i++;
        }
        string stderrText = "";
        int j = 0;
        while (j < stderrLines.Count) {
            if (stderrText.Length > 0) { stderrText = stderrText + "\n"; }
            stderrText = stderrText + stderrLines[j];
            j++;
        }
        ProcessRunResult result = new ProcessRunResult();
        result.ExitCode = p.ExitCode;
        result.StandardOutput = stdoutText;
        result.StandardError = stderrText;
        result.TimedOut = !exited;
        ProcessRunStats? stats = p.GetRunStats();
        if (stats != null) {
            stats.ElapsedMs = sw.ElapsedMilliseconds;
            result.Stats = stats;
        }
        p.Dispose();
        return result;
    }

    /// <summary>异步启动进程。</summary>
    public async Task StartAsync(CancellationToken cancellationToken = default) {
        cancellationToken.ThrowIfCancellationRequested();
        this.Start();
    }

    /// <summary>异步等待进程退出，返回真实退出码。</summary>
    public async Task<int> WaitForExitAsync(CancellationToken cancellationToken = default) {
        cancellationToken.ThrowIfCancellationRequested();
        // Task.Run 卸载到线程池：this 按引用捕获，退出后读取真实退出码。
        // 不能用 int 局部变量中转——值类型按值捕获，闭包内赋值对外层不可见。
        await Task.Run(() => this.WaitForExit());
        return this.ExitCode;
    }

    /// <summary>异步捕获进程输出。</summary>
    public static async Task<ProcessRunResult> RunCaptureAsync(ProcessStartInfo startInfo, CancellationToken cancellationToken = default) {
        cancellationToken.ThrowIfCancellationRequested();
        return Process.RunCapture(startInfo);
    }

    /// <summary>异步捕获进程输出（RFC 043 P1 超时版）：<paramref name="timeoutMs"/> 内未退出 → Kill → TimedOut 标记。</summary>
    public static async Task<ProcessRunResult> RunCaptureAsync(ProcessStartInfo startInfo, int timeoutMs, CancellationToken cancellationToken = default) {
        cancellationToken.ThrowIfCancellationRequested();
        return Process.RunCapture(startInfo, timeoutMs);
    }

    public static int GetCurrentProcessId() {
        return rt_process.rt_proc_get_current_pid();
    }

    public static ProcessStreamSession StartStreaming(ProcessStartInfo startInfo) {
        Process p = Process.Start(startInfo);
        return new ProcessStreamSession(p);
    }
}
