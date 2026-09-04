// ReplFixRoundProvider —— L2 自动迭代的 REPL 修复回合：把结构化失败回喂作为一轮模型回合
// （含 HITL 审批回环）。机器闭环真实路径：/dod D0–D3 失败 → 回喂 → 模型修码 → 重跑门。
namespace ArcAgent.Repl;
using Arc;
using Arc.Agent;
using Arc.Agent.Harness;

/// <summary>
/// 以 <see cref="AISession"/> 模型回合实现 <see cref="IAIFixRoundProvider"/>：一轮 FixAsync =
/// 提交「修复以下验证失败」prompt → 模型走工具（read_file/edit_file/arc_build/arc_test）
/// 修码 → 返回模型回复作为修复说明。写工具须 HITL 审批（同 REPL 回合审批回环）。
/// </summary>
public class ReplFixRoundProvider : IAIFixRoundProvider {
    private AISession _session;

    public ReplFixRoundProvider(AISession session) {
        if (session == null) {
            throw new ArgumentException("session is required");
        }
        _session = session;
    }

    /// <summary>一轮修复：以失败回喂为 prompt 跑模型回合（含 HITL 回环），返回修复说明。</summary>
    public async Task<string> FixAsync(AIDoDFixFeedback feedback, CancellationToken cancellationToken) {
        string prompt = "DoD verification failed (round " + feedback.RoundNumber + "). "
            + "Investigate the failure, fix the root cause, and re-verify (≤3 fix rounds).\n\n"
            + feedback.Describe()
            + "\n\nFix the code now and report what you changed and the verification result.";
        AIReply reply = await _session.RunAsync(prompt, cancellationToken);
        while (reply != null && reply.NeedsHuman) {
            await this.HandleApprovalAsync(reply, cancellationToken);
            reply = await _session.ResumeAsync(cancellationToken);
        }
        if (reply == null) {
            return "(fix round produced no reply)";
        }
        if (reply.IsError) {
            return "(fix round error: " + reply.ErrorKind + ": " + reply.ErrorMessage + ")";
        }
        return reply.Text != null && reply.Text != "" ? reply.Text : "(no text output)";
    }

    /// <summary>HITL 审批回环（与 ReplSession 回合审批同构：展示门闩 → 批准/编辑/拒绝 → 继续）。</summary>
    private async Task HandleApprovalAsync(AIReply reply, CancellationToken cancellationToken) {
        AIHumanRequest gate = reply.Gate != null ? reply.Gate : _session.PendingHuman;
        Console.WriteLine("\n[approval] tool=" + (gate != null ? gate.ToolName : "?"));
        Console.WriteLine("[approval] args=" + (gate != null ? gate.ToolArguments : "?"));
        Console.Write("approve? [a]pprove / [e]dit args / [r]eject: ");
        string choice = Console.ReadLine();
        if (choice == null || choice == "") {
            choice = "r";
        }
        if (choice == "e" && gate != null) {
            Console.Write("  new args (JSON): ");
            string newArgs = Console.ReadLine();
            await _session.ApproveAsync(
                new AIToolCall(gate.ToolCallId, gate.ToolName, newArgs != null ? newArgs : ""),
                cancellationToken);
        } else if (choice == "r") {
            await _session.RejectAsync("user rejected", cancellationToken);
        } else if (gate != null) {
            await _session.ApproveAsync(
                new AIToolCall(gate.ToolCallId, gate.ToolName, gate.ToolArguments),
                cancellationToken);
        }
    }
}
