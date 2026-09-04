// ProcessRunner —— 进程执行封装：跨平台 shell 包装 + 输出捕获（run_command / git 查询统一底座）。
//
// 消除 ShellTools / RepoTools / AgentWorkspace 三处重复的 cmd.exe / sh -c 分支：
// 职责边界（分层：Process 层）——只负责「执行命令 + 捕获 stdout/stderr/exit」，不含任何业务语义。
namespace ArcAgent.Process;
using Arc;
using Arc.Diagnostics;

/// <summary>跨平台进程执行（cmd.exe / sh -c 包装，捕获输出）。</summary>
public static class ProcessRunner {
    /// <summary>执行 shell 命令并捕获输出；空命令抛 <see cref="ArgumentException"/>。</summary>
    public static async Task<ProcessRunResult> RunAsync(string command) {
        if (command == null || command == "") {
            throw new ArgumentException("command is empty");
        }
        ProcessStartInfo si = new ProcessStartInfo();
        if (Environment.IsWindows()) {
            si.FileName = "cmd.exe";
            si.Arguments = "/c " + command;
        } else {
            si.FileName = "/bin/sh";
            si.Arguments = "-c " + command;
        }
        return await Process.RunCaptureAsync(si, new CancellationToken());
    }
}
