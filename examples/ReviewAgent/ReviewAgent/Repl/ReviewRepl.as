// ReviewRepl —— 领域二控制台交互（薄壳）：方向环命令经 AIHarnessSession 走基座，
// 领域工具直测；不重复实现 PM / DoD / 事件日志。
namespace ReviewAgent.Repl;
using Arc;
using Arc.Agent;
using Arc.Agent.Harness;
using ReviewAgent.Tools;

/// <summary>控制台交互层：REPL 循环 + 方向环命令委托 + 领域工具直测。</summary>
public class ReviewRepl {
    private AIHost _host;
    private AISession _session;
    private AIHarnessSession _harness;
    private string _workspace;
    private ReviewTools _tools;

    public ReviewRepl(AIHost host, AISession session, AIHarnessSession harness, string workspace) {
        _host = host;
        _session = session;
        _harness = harness;
        _workspace = workspace != null ? workspace : ".";
        _tools = new ReviewTools();
    }

    /// <summary>运行 REPL：读输入 → 命令路由 / 任务回合；exit/quit 退出。</summary>
    public async Task RunAsync() {
        this.PrintBanner();
        bool running = true;
        while (running) {
            Console.Write("\n> ");
            string input = Console.ReadLine();
            if (input == null || input == "exit" || input == "quit") {
                running = false;
                continue;
            }
            string trimmed = input.Trim();
            if (trimmed == "" || trimmed == "help" || trimmed == "?") {
                this.PrintHelp();
                continue;
            }
            if (trimmed.StartsWith("/rfc ")) {
                this.HandleSetRfc(trimmed.Substring("/rfc ".Length).Trim());
                continue;
            }
            if (trimmed == "/revise") {
                this.HandleRevise();
                continue;
            }
            if (trimmed.StartsWith("/summary ")) {
                this.HandleSummary(trimmed.Substring("/summary ".Length).Trim());
                continue;
            }
            if (trimmed.StartsWith("/checkpoint ")) {
                await this.HandleCheckpointAsync(trimmed.Substring("/checkpoint ".Length).Trim());
                continue;
            }
            if (trimmed.StartsWith("/rollback ")) {
                await this.HandleRollbackAsync(trimmed.Substring("/rollback ".Length).Trim());
                continue;
            }
            if (trimmed == "/dod") {
                await this.HandleDoDAsync();
                continue;
            }
            if (trimmed.StartsWith("/review ")) {
                Console.WriteLine(_tools.ReviewFile(trimmed.Substring("/review ".Length).Trim()));
                continue;
            }
            if (trimmed.StartsWith("/check")) {
                string folder = trimmed.Substring("/check".Length).Trim();
                if (folder == "") {
                    folder = _workspace;
                }
                Console.WriteLine(_tools.CheckConsistency(folder));
                continue;
            }
            await this.RunTurnAsync(trimmed);
        }
    }

    private void HandleSetRfc(string id) {
        string rfcId = id != "" ? id : "REVIEW-RFC";
        AIAcceptanceSpec acc = new AIAcceptanceSpec();
        acc.Assertions = "all cross-references resolve";
        AIRfc? rfc = _harness.SetRfc(
            rfcId,
            new AIIntentionSpec("review the document set"),
            new AIDesignSpec(),
            acc);
        if (rfc == null) {
            Console.WriteLine("rfc rejected (lease conflict)");
            return;
        }
        Console.WriteLine("rfc v" + rfc.Revision + " created (" + rfc.RfcId + "), events=" + _session.DecisionEvents.Count);
    }

    private void HandleRevise() {
        AIRfc? current = _harness.Rfc;
        if (current == null) {
            Console.WriteLine("no active rfc — use /rfc <id> first");
            return;
        }
        AIAcceptanceSpec acc = new AIAcceptanceSpec();
        acc.Assertions = "all cross-references resolve";
        AIRfc? next = _harness.ReviseRfc(
            new AIIntentionSpec("review the document set (revised)"),
            new AIDesignSpec(),
            acc,
            "revised acceptance");
        if (next == null) {
            Console.WriteLine("revise rejected (lease conflict)");
            return;
        }
        Console.WriteLine("rfc v" + next.Revision + " (" + next.RfcId + ")");
    }

    private void HandleSummary(string label) {
        if (label == "") {
            label = "review-unit";
        }
        _harness.RecordSummary(new AIWorkSummary(label, "document review", "需求 ✓", "consistency check"));
        Console.WriteLine("summary recorded (work_summary), events=" + _session.DecisionEvents.Count);
    }

    private async Task HandleCheckpointAsync(string label) {
        bool captured = await _harness.CheckpointGreenAsync(label != "" ? label : "green", new CancellationToken());
        Console.WriteLine(captured ? "checkpoint captured" : "checkpoint skipped (snapshot:none)");
    }

    private async Task HandleRollbackAsync(string reason) {
        bool ok = await _harness.CheckpointRollbackAsync("rollback", reason != "" ? reason : "user rollback", new CancellationToken());
        Console.WriteLine(ok ? "rollback ok" : "rollback failed (no checkpoint)");
    }

    /// <summary>跑自动门（D0→D7）；未接线门由 evaluator 诚实 Pending；D5/D7 人类门待确认。</summary>
    private async Task HandleDoDAsync() {
        AIRfc? rfc = _harness.Rfc;
        if (rfc == null) {
            Console.WriteLine("no active rfc — use /rfc <id> first");
            return;
        }
        List<AIDoDGateResult> results = await _harness.DoD.RunAutoGatesAsync(rfc, new CancellationToken());
        int i = 0;
        while (i < results.Count) {
            AIDoDGateResult r = results[i];
            Console.WriteLine("[dod] " + ReviewRepl.GateName(r.Gate) + " = " + ReviewRepl.StatusName(r.Status) + " (" + r.Signal + ")");
            i = i + 1;
        }
        Console.WriteLine("all_passed=" + (AIDoDOrchestrator.AllPassed(results) ? "true" : "false"));
    }

    /// <summary>单轮任务：模型回合（经会话；NeedsHuman → 简单审批回环）。</summary>
    private async Task RunTurnAsync(string input) {
        AIReply reply = await _session.RunAsync(input, new CancellationToken());
        while (reply != null && reply.NeedsHuman) {
            AIHumanRequest? gate = reply.Gate;
            Console.WriteLine("[approval] tool=" + (gate != null ? gate.ToolName : "?"));
            Console.Write("approve? [a]pprove / [r]eject: ");
            string choice = Console.ReadLine();
            if (choice == null || choice == "" || choice == "r") {
                await _session.RejectAsync("user rejected", new CancellationToken());
            } else if (gate != null) {
                await _session.ApproveAsync(
                    new AIToolCall(gate.ToolCallId, gate.ToolName, gate.ToolArguments),
                    new CancellationToken());
            }
            reply = await _session.ResumeAsync(new CancellationToken());
        }
        this.PrintReply(reply);
    }

    private void PrintReply(AIReply reply) {
        if (reply == null) {
            Console.WriteLine("[error] null reply");
            return;
        }
        if (reply.IsError) {
            Console.WriteLine("[error] " + (reply.ErrorKind != null ? reply.ErrorKind : "") + ": " + (reply.ErrorMessage != null ? reply.ErrorMessage : ""));
            return;
        }
        if (reply.Text != null && reply.Text != "") {
            Console.WriteLine("[reply] " + reply.Text);
        }
    }

    private void PrintBanner() {
        Console.WriteLine("");
        Console.WriteLine("ReviewAgent ready (domain two: document review).");
        Console.WriteLine("  workspace: " + _workspace);
        Console.WriteLine("  commands: help /rfc <id> /revise /summary <label> /checkpoint <label> /rollback <reason> /dod /review <file> /check [folder] /exit");
    }

    private void PrintHelp() {
        Console.WriteLine("  /rfc <id>            成立 AIRfc（Revision 1）");
        Console.WriteLine("  /revise              升版纠偏（Revision+1）");
        Console.WriteLine("  /summary <label>     记录工作单元小结（work_summary）");
        Console.WriteLine("  /checkpoint <label>  绿点快照（checkpoint:green）");
        Console.WriteLine("  /rollback <reason>   回滚最近绿点（checkpoint:rollback）");
        Console.WriteLine("  /dod                 跑 D0–D7 自动门（领域判定）");
        Console.WriteLine("  /review <file>       单文档审查（行数 + TODO/FIXME）");
        Console.WriteLine("  /check [folder]      交叉引用一致性检查");
        Console.WriteLine("  <text>               模型回合；exit/quit 退出");
    }

    private static string GateName(AIDoDGateKind gate) {
        if (gate == AIDoDGateKind.D0Compile) { return "D0"; }
        if (gate == AIDoDGateKind.D1Semantics) { return "D1"; }
        if (gate == AIDoDGateKind.D2Contract) { return "D2"; }
        if (gate == AIDoDGateKind.D3Behavior) { return "D3"; }
        if (gate == AIDoDGateKind.D4DiffCoverage) { return "D4"; }
        if (gate == AIDoDGateKind.D5SelfReview) { return "D5"; }
        if (gate == AIDoDGateKind.D6AntiPattern) { return "D6"; }
        return "D7";
    }

    private static string StatusName(AIDoDGateStatus status) {
        if (status == AIDoDGateStatus.Pending) { return "Pending"; }
        if (status == AIDoDGateStatus.Passed) { return "Passed"; }
        if (status == AIDoDGateStatus.Failed) { return "Failed"; }
        return "NeedsHuman";
    }
}
