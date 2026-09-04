// M8 CodeAct（RFC 038 §3.4.2）：脚本后端——独立解释器进程执行。
//
// 进程级沙箱：模型代码在独立子进程（如 python）中跑，绝不在宿主进程内执行任意代码。
// 配额：超时（WaitForExit(timeout) 超时 → Kill）+ 输出上限（截断标记 Truncated）。
// 取消：CancellationToken.Register → Kill 子进程。
//
// 注：env 为接口契约预留——当前 runtime `rt_proc_spawn` 不注入环境（CreateProcessW 继承
// 父环境），故本实现暂不注入，亦不静默声称生效；未来 runtime 增强后在此补透传。
namespace Arc.Agent;
using Arc;
using Arc.Diagnostics;
using Arc.Threading;
using Arc.Collections;

/// <summary>
/// 脚本 CodeAct 后端。以独立解释器进程执行代码——进程级沙箱 + 超时/取消终止 + 输出截断。
/// 解释器经 stdin 接收代码（`-` 参数，读到 EOF 开始执行），stdout/stderr 并发回收。
/// </summary>
public class AIProcessCodeActProvider : IAICodeActProvider {
    private string _args;

    /// <summary>以解释器 + 默认 `-`（stdin 读码）参数创建。</summary>
    public AIProcessCodeActProvider(string interpreter) {
        this.Interpreter = interpreter != null ? interpreter : "";
        _args = "-";
    }

    /// <summary>以解释器 + 显式参数创建（如 "python" / "-c"）。</summary>
    public AIProcessCodeActProvider(string interpreter, string args) {
        this.Interpreter = interpreter != null ? interpreter : "";
        _args = args != null ? args : "-";
    }

    public string Interpreter { get; }

    public string? WorkingDirectory { get; set; }

    public async Task<AICodeActResult> ExecuteAsync(
        string code,
        Dictionary<string, string?> env,
        long timeoutMs,
        int maxOutputChars,
        CancellationToken cancellationToken) {
        if (this.Interpreter == "") {
            return AICodeActResult.Fail("no interpreter configured");
        }
        if (cancellationToken.IsCancellationRequested) {
            AICodeActResult pre = new AICodeActResult();
            pre.Cancelled = true;
            pre.Error = "cancelled before start";
            return pre;
        }
        long start = Stopwatch.GetTimestamp();

        ProcessStartInfo psi = new ProcessStartInfo();
        psi.FileName = this.Interpreter;
        psi.Arguments = _args;
        psi.RedirectStandardOutput = true;
        psi.RedirectStandardError = true;
        psi.CreateNoWindow = true;
        if (this.WorkingDirectory != null) {
            psi.WorkingDirectory = this.WorkingDirectory;
        }
        // 经共享捕获基座执行（AG-12）：并发读 stdout/stderr、取消/超时 Kill、输出截断。
        // stdin 承载代码（解释器读至 EOF 开始执行）。
        AICodeActResult r = new AICodeActResult();
        bool started = CodeActProcessRunner.RunCaptured(
            psi, code != null ? code : "", timeoutMs, maxOutputChars, cancellationToken, r);
        if (!started) {
            return AICodeActResult.Fail("failed to start interpreter: " + this.Interpreter);
        }
        r.DurationMs = CodeActProcessRunner.ElapsedMs(start);
        if (r.TimedOut) {
            r.Error = "timeout after " + timeoutMs + "ms";
        } else if (r.Cancelled) {
            r.Error = "cancelled";
        }
        return r;
    }
}

/// <summary>
/// CodeAct 进程捕获共享基座（internal）：脚本后端与原生后端复用同一套并发读 stdout/stderr、
/// 取消/超时 Kill、输出截断与耗时换算，消除逐字重复（AG-12）。
/// </summary>
internal static class CodeActProcessRunner {
    /// <summary>运行一个进程并捕获输出（超时/取消 Kill + 截断）。res 接收结果；返回是否成功启动。</summary>
    internal static bool RunCaptured(
        ProcessStartInfo psi,
        string? stdin,
        long timeoutMs,
        int maxOutputChars,
        CancellationToken cancellationToken,
        AICodeActResult res) {
        Process p = new Process();
        p.StartInfo = psi;
        if (stdin != null) {
            psi.RedirectStandardInput = true;
        }
        try {
            p.Start();
        } catch (Exception) {
            p.Dispose();
            return false;
        }
        if (stdin != null) {
            p.StandardInput.WriteString(stdin);
            p.StandardInput.Dispose();
        }
        // 并发读 stdout/stderr（List<string> 引用类型捕获可正确回写；避免管道缓冲死锁）。
        List<string> outLines = new List<string>();
        List<string> errLines = new List<string>();
        Thread ot = new Thread(() => {
            string? l = p.StandardOutput.ReadLine();
            while (l != null) {
                outLines.Add(l);
                l = p.StandardOutput.ReadLine();
            }
        });
        Thread et = new Thread(() => {
            string? l = p.StandardError.ReadLine();
            while (l != null) {
                errLines.Add(l);
                l = p.StandardError.ReadLine();
            }
        });
        ot.Start();
        et.Start();
        // 事件驱动取消：取消时终止子进程（编译器已建模 Register/CanBeCanceled）。
        if (cancellationToken.CanBeCanceled) {
            cancellationToken.Register(() => {
                if (!p.HasExited) {
                    p.Kill();
                }
            });
        }
        // 阻塞等待：超时到期或取消回调主动 Kill → WaitForExit 提前返回；超时须主动 Kill。
        bool timedOut = false;
        if (timeoutMs > 0) {
            timedOut = !p.WaitForExit((int)timeoutMs);
        }
        if (timedOut) {
            p.Kill();
        }
        p.WaitForExit();
        ot.Join();
        et.Join();
        res.ExitCode = p.ExitCode;
        res.StandardOutput = CodeActProcessRunner.JoinTruncated(outLines, maxOutputChars);
        res.StandardError = CodeActProcessRunner.JoinTruncated(errLines, maxOutputChars);
        res.TimedOut = timedOut;
        res.Cancelled = cancellationToken.IsCancellationRequested;
        res.Truncated = CodeActProcessRunner.IsTruncated(outLines, maxOutputChars)
            || CodeActProcessRunner.IsTruncated(errLines, maxOutputChars);
        res.Success = res.ExitCode == 0 && !timedOut && !res.Cancelled;
        p.Dispose();
        return true;
    }

    /// <summary>行列表拼装为单文本；超出上限字符截断（配合 IsTruncated 判定）。</summary>
    internal static string JoinTruncated(List<string> lines, int maxChars) {
        string text = "";
        int n = lines.Count;
        int i = 0;
        while (i < n) {
            string line = lines[i];
            if (text.Length > 0) { text = text + "\n"; }
            if (maxChars > 0 && text.Length + line.Length > maxChars) {
                int room = maxChars - text.Length;
                if (room > 0) {
                    text = text + line.Substring(0, room);
                }
                return text;
            }
            text = text + line;
            i = i + 1;
        }
        return text;
    }

    /// <summary>判定拼接后是否超上限（与 JoinTruncated 同规则）。</summary>
    internal static bool IsTruncated(List<string> lines, int maxChars) {
        if (maxChars <= 0) { return false; }
        int total = 0;
        int n = lines.Count;
        int i = 0;
        while (i < n) {
            if (i > 0) { total = total + 1; }
            total = total + lines[i].Length;
            if (total > maxChars) { return true; }
            i = i + 1;
        }
        return false;
    }

    /// <summary>自起始时间戳起的耗时（毫秒）。</summary>
    internal static long ElapsedMs(long startTicks) {
        return (Stopwatch.GetTimestamp() - startTicks) * 1000 / Stopwatch.Frequency;
    }
}
