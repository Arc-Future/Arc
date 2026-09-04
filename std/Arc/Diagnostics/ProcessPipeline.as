// Arc.Diagnostics.ProcessPipeline — Unix 风格管道组合（并发流式）。

namespace Arc.Diagnostics;

using Arc.Collections;
using Arc.Threading;

/// <summary>
/// Unix 风格管道组合——将多个进程串联，前一个 stdout 逐行流式泵送到后一个 stdin。
/// 所有阶段并发启动，后台线程泵送中间数据，避免全量内存中转和死锁。
/// </summary>
public class ProcessPipeline {
    private List<ProcessStartInfo> _stages;

    public ProcessPipeline() {
        _stages = new List<ProcessStartInfo>();
    }

    public ProcessPipeline Add(ProcessStartInfo stage) {
        _stages.Add(stage);
        return this;
    }

    public ProcessRunResult Run() {
        if (_stages.Count == 0) {
            throw new InvalidOperationException("Pipeline is empty");
        }

        // 启动所有进程（最后一个捕获 stdout+stderr）
        List<Process> processes = new List<Process>();
        for (int i = 0; i < _stages.Count; i++) {
            ProcessStartInfo si = _stages[i];
            if (i < _stages.Count - 1) {
                si.RedirectStandardOutput = true;
            }
            if (i > 0) {
                si.RedirectStandardInput = true;
            }
            if (i == _stages.Count - 1) {
                si.RedirectStandardOutput = true;
                si.RedirectStandardError = true;
            }
            processes.Add(Process.Start(si));
        }

        // 后台线程逐行泵送：阶段 N stdout → 阶段 N+1 stdin
        List<Thread> pumps = new List<Thread>();
        for (int i = 0; i < processes.Count - 1; i++) {
            Process src = processes[i];
            Process to = processes[i + 1];
            Thread pump = new Thread(() => {
                string? line = src.StandardOutput.ReadLine();
                while (line != null) {
                    to.StandardInput.WriteLine(line);
                    line = src.StandardOutput.ReadLine();
                }
                to.StandardInput.Dispose();
            });
            pumps.Add(pump);
            pump.Start();
        }

        // 等最后一个进程的 stdout 读完
        string stdoutText = "";
        string? outLine = processes[processes.Count - 1].StandardOutput.ReadLine();
        while (outLine != null) {
            if (stdoutText.Length > 0) { stdoutText = stdoutText + "\n"; }
            stdoutText = stdoutText + outLine;
            outLine = processes[processes.Count - 1].StandardOutput.ReadLine();
        }

        // 等所有泵送线程完成
        for (int i = 0; i < pumps.Count; i++) {
            pumps[i].Join();
        }

        // 等所有进程退出
        for (int i = 0; i < processes.Count; i++) {
            processes[i].WaitForExit();
        }

        ProcessRunResult result = new ProcessRunResult();
        result.ExitCode = processes[processes.Count - 1].ExitCode;
        result.StandardOutput = stdoutText;
        result.StandardError = "";
        for (int i = 0; i < processes.Count; i++) {
            processes[i].Dispose();
        }
        return result;
    }

    public ProcessStreamSession RunStreaming() {
        if (_stages.Count == 0) {
            throw new InvalidOperationException("Pipeline is empty");
        }

        List<Process> processes = new List<Process>();
        for (int i = 0; i < _stages.Count; i++) {
            ProcessStartInfo si = _stages[i];
            if (i < _stages.Count - 1) {
                si.RedirectStandardOutput = true;
            }
            if (i > 0) {
                si.RedirectStandardInput = true;
            }
            if (i == _stages.Count - 1) {
                si.RedirectStandardOutput = true;
                si.RedirectStandardError = true;
            }
            processes.Add(Process.Start(si));
        }

        // 后台泵送
        for (int i = 0; i < processes.Count - 1; i++) {
            Process src = processes[i];
            Process to = processes[i + 1];
            Thread pump = new Thread(() => {
                string? line = src.StandardOutput.ReadLine();
                while (line != null) {
                    to.StandardInput.WriteLine(line);
                    line = src.StandardOutput.ReadLine();
                }
                to.StandardInput.Dispose();
            });
            pump.Start();
        }

        return new ProcessStreamSession(processes[processes.Count - 1]);
    }
}
