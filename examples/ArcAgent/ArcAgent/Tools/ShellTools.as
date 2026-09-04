// ShellTools —— shell 命令工具（shell.Run；RequireApproval=true 触发 HITL 门闩）。
//
// RFC 038 §5.1.3：工具按职责范围分类（FS 一组、shell 一组），每类一文件。
// 计划门闩：fs.Write/shell.Run 由 AgentHost.PlanGatedCapabilities 声明受约束，调度层
// （AIToolSandbox + AIPlanGate）统一拦截未批准计划的写入，工具本层无需手写门闩。
namespace ArcAgent.Tools;
using Arc;
using Arc.Agent;
using Arc.ComponentModel;
using Arc.Diagnostics;
using ArcAgent.Process;

/// <summary>shell 命令工具集（run_command）。</summary>
public class ShellTools {
    /// <summary>执行 shell 命令并捕获 stdout/stderr/exit code（shell.Run，RequireApproval）。</summary>
    [Description("Run a shell command and capture its stdout/stderr and exit code. Requires human approval.")]
    [AITool("run_command", Capability = "shell.Run", RequireApproval = true)]
    public async Task<string> RunCommandAsync([Description("The shell command to execute.")] string command) {
        ProcessRunResult r = await ProcessRunner.RunAsync(command);
        string output = "exit=" + r.ExitCode;
        if (r.StandardOutput != "") {
            output = output + "\nSTDOUT:\n" + r.StandardOutput;
        }
        if (r.StandardError != "") {
            output = output + "\nSTDERR:\n" + r.StandardError;
        }
        return output;
    }
}