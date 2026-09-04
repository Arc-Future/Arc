// ReplSession —— REPL 主循环 + 任务回合（RunAsync + HITL 门闩 + 会话事件日志）。
//
// 职责边界（分层：Repl 层）：
//   - RunAsync：控制台循环（读输入 → 命令路由 / 任务回合分发）。
//   - RunTurnAsync：单轮任务（日志用户消息 → RunAsync → HITL 回环 → 落工具/回复/用量事件）。
//   - 事件缓冲：工具/用量钩子为同步回调，先入缓冲，回合结束统一落日志（保证事件序 = 发生序）。
// 斜杠命令委托 ReplCommands；不含业务/装配逻辑。
namespace ArcAgent.Repl;
using Arc;
using Arc.Agent;
using Arc.Agent.Harness;
using ArcAgent.Context;
using ArcAgent.SessionLog;
using ArcAgent.Workspace;

/// <summary>控制台交互层：REPL 循环 + 任务回合调度 + 会话事件记录。</summary>
public class ReplSession {
    private AIHost _host;
    private AISession _session;
    private AgentContext _context;
    private AgentWorkspace _workspace;
    private SessionEventLog _log;
    private ReplCommands _commands;
    private List<SessionEvent> _toolBuffer;
    private List<AITokenUsage> _usageBuffer;

    public ReplSession(AIHost host, AISession session, AgentContext context, AgentWorkspace workspace,
        SessionEventLog log, AIHarnessSession harness) {
        _host = host;
        _session = session;
        _context = context;
        _workspace = workspace;
        _log = log;
        _commands = new ReplCommands(host, session, context, log, workspace, harness);
        _toolBuffer = new List<SessionEvent>();
        _usageBuffer = new List<AITokenUsage>();
    }

    /// <summary>运行 REPL：读输入 → 命令路由 / 任务回合；exit/quit 退出。</summary>
    public async Task RunAsync() {
        this.SetupHooks();
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
            if (trimmed == "") {
                continue;
            }
            if (trimmed.StartsWith("/task ")) {
                string taskPrompt = trimmed.Substring("/task ".Length).Trim();
                if (taskPrompt == "") {
                    Console.WriteLine("usage: /task <prompt> — start an autonomous multi-turn task run");
                } else {
                    await this.RunTaskAsync(taskPrompt);
                }
                continue;
            }
            bool handled = await _commands.TryHandleAsync(trimmed);
            if (!handled) {
                await this.RunTurnAsync(trimmed);
            }
        }
    }

    /// <summary>装配会话事件钩子：流式文本 + 工具生命周期 + 用量上报（同步回调 → 缓冲）。</summary>
    private void SetupHooks() {
        _session.TextDelta = (d: string) => { Console.Write(d); };
        _session.ToolInvoked = (c: AIToolCall) => { Console.WriteLine("\n[tool] " + c.Name); };
        _session.ToolCompleted = (c: AIToolCall, r: AIToolResult) => {
            _toolBuffer.Add(SessionEvent.Tool(
                c != null ? c.Name : "",
                c != null ? c.ArgumentsJson : "",
                r != null && r.IsError,
                r != null ? r.Content : ""));
            if (r != null && r.IsError) {
                Console.WriteLine("  -> error [" + r.ErrorKind + "] " + r.Content);
            }
        };
        _session.UsageReported = (u: AITokenUsage) => {
            if (u != null) {
                _usageBuffer.Add(u);
            }
        };
        // 决策轨迹（RFC 043 M5–M6 单轨）：Harness 的 airfc:*/checkpoint:* 经 Agent 会话事件面
        // 上报 → 落既有 JSONL 日志（事件回调为同步，与计划事件订阅同一 fire-and-forget 模式）。
        _session.DecisionEventReported = (e: AIDecisionEvent) => {
            if (e != null) {
                _log.AppendAsync(
                    SessionEvent.Decision(AIDecisionEventKindCodec.ToWireString(e.Kind), e.Detail, e.Reason),
                    new CancellationToken());
            }
        };
    }

    private void PrintBanner() {
        Console.WriteLine("");
        Console.WriteLine("ArcAgent ready.");
        Console.WriteLine("  workspace: " + _workspace.Root);
        Console.WriteLine("  memory pages: " + ("" + _context.KnowledgePaths(_host.Wiki).Count));
        Console.WriteLine("  commands: /help /plan /approve /reject /memory /remember <path> <text> /forget <path> /usage /clear /sessions /resume [id] /task <prompt> /run <需求> /rfc /revise /summary /checkpoint /rollback /dod /exit");
    }

    /// <summary>单轮任务：用户消息入日志 → RunAsync → HITL 回环 → 统一落工具/回复/用量事件。</summary>
    private async Task RunTurnAsync(string input) {
        await _log.AppendAsync(SessionEvent.User(input), new CancellationToken());
        AIReply reply = await _session.RunAsync(input, new CancellationToken());
        while (reply != null && reply.NeedsHuman) {
            await this.HandleApprovalAsync(reply);
            reply = await _session.ResumeAsync(new CancellationToken());
        }
        // 先落工具事件（工具先于最终回复），再落助手回复/错误，最后落用量。
        await this.FlushToolEventsAsync();
        await this.PrintReplyAsync(reply);
        await this.FlushUsageEventsAsync();
    }

    /// <summary>
    /// 自主多回合任务（/task <prompt>）：AITaskRun 编排闭环——有界回合推进（MaxSteps）+ 时长熔断
    /// + HITL 门闩（NeedsHuman → 审批回环）+ 进度可观测。每轮模型有产出（文本）即自驱动下一轮
    /// （对齐 AITaskRun 有界编排契约），直至无产出 / 出错 / 超时 / 达步数上限。
    /// </summary>
    private async Task RunTaskAsync(string prompt) {
        AITaskRun task = new AITaskRun(_session, 25);
        task.OnProgress = (p: string) => { Console.WriteLine("[task] " + p); };
        task.OnStateChanged = (s: string) => { Console.WriteLine("[task] state=" + s); };
        task.Start();
        string current = prompt;
        while (task.Status == AITaskRunStatus.Running) {
            if (task.IsDurationExceeded()) {
                task.Fail("TaskTimeout");
                break;
            }
            AIReply reply = await task.RunStepAsync(current, new CancellationToken());
            while (reply != null && reply.NeedsHuman) {
                await this.HandleApprovalAsync(reply);
                reply = await _session.ResumeAsync(new CancellationToken());
            }
            await this.FlushToolEventsAsync(); // 回合内工具事件先落日志
            await this.FlushUsageEventsAsync();
            if (reply == null || reply.IsError) {
                task.Fail(reply != null && reply.ErrorKind != "" ? reply.ErrorKind : "Error");
                if (reply != null && reply.ErrorMessage != "") {
                    await _log.AppendAsync(SessionEvent.Error(reply.ErrorKind, reply.ErrorMessage), new CancellationToken());
                }
                break;
            }
            if (reply.Text == null || reply.Text == "") {
                break; // 无产出（纯工具轮已结束）→ 任务完成
            }
            await _log.AppendAsync(SessionEvent.Assistant(reply.Text), new CancellationToken());
            Console.WriteLine("\n[task] " + reply.Text);
            current = reply.Text; // 有产出即自驱动下一轮
        }
        if (task.Status == AITaskRunStatus.Running) {
            task.Complete();
        }
        Console.WriteLine("[task] done: " + this.TaskStatusName(task.Status) + " (" + task.Steps + "/" + task.MaxSteps + " steps)");
    }

    /// <summary>把缓冲的工具事件落日志（换新列表规避迭代器失效；保持发生序）。</summary>
    private async Task FlushToolEventsAsync() {
        List<SessionEvent> pending = _toolBuffer;
        _toolBuffer = new List<SessionEvent>();
        foreach (var evt in pending) {
            await _log.AppendAsync(evt, new CancellationToken());
        }
    }

    /// <summary>把缓冲的用量事件落日志（换新列表；保持发生序）。</summary>
    private async Task FlushUsageEventsAsync() {
        List<AITokenUsage> pendingUsage = _usageBuffer;
        _usageBuffer = new List<AITokenUsage>();
        foreach (var u in pendingUsage) {
            await _log.AppendAsync(SessionEvent.Usage(u.PromptTokens, u.CompletionTokens, u.TotalTokens), new CancellationToken());
        }
    }

    private string TaskStatusName(AITaskRunStatus s) {
        if (s == AITaskRunStatus.Completed) { return "completed"; }
        if (s == AITaskRunStatus.Failed) { return "failed"; }
        if (s == AITaskRunStatus.Cancelled) { return "cancelled"; }
        if (s == AITaskRunStatus.Paused) { return "paused"; }
        if (s == AITaskRunStatus.Running) { return "running"; }
        return "pending";
    }

    /// <summary>HITL 审批：展示门闩 → 用户批准/编辑/拒绝 → 决策入日志。</summary>
    private async Task HandleApprovalAsync(AIReply reply) {
        AIHumanRequest gate = reply.Gate != null ? reply.Gate : _session.PendingHuman;
        Console.WriteLine("\n[approval] tool=" + (gate != null ? gate.ToolName : "?"));
        Console.WriteLine("[approval] args=" + (gate != null ? gate.ToolArguments : "?"));
        Console.Write("approve? [a]pprove / [e]dit args / [r]eject: ");
        string choice = Console.ReadLine();
        if (choice == null || choice == "") {
            choice = "r";
        }
        string decision = "rejected";
        string reason = "user rejected";
        if (choice == "e" && gate != null) {
            Console.Write("  new args (JSON): ");
            string newArgs = Console.ReadLine();
            await _session.ApproveAsync(new AIToolCall(gate.ToolCallId, gate.ToolName, newArgs != null ? newArgs : ""), new CancellationToken());
            decision = "approved edited";
            reason = "user edited args";
        } else if (choice == "r") {
            await _session.RejectAsync("user rejected", new CancellationToken());
        } else if (gate != null) {
            await _session.ApproveAsync(new AIToolCall(gate.ToolCallId, gate.ToolName, gate.ToolArguments), new CancellationToken());
            decision = "approved";
            reason = "user approved";
        }
        if (gate != null) {
            await _log.AppendAsync(SessionEvent.Approval(gate.ToolName, decision, reason), new CancellationToken());
        }
    }

    /// <summary>展示回合结果并落对应事件（null/错误 → Error 事件，正常回复 → Assistant 事件）。</summary>
    private async Task PrintReplyAsync(AIReply reply) {
        if (reply == null) {
            await _log.AppendAsync(SessionEvent.Error("NullReply", "null reply from session"), new CancellationToken());
            Console.WriteLine("\n[error] null reply");
            return;
        }
        if (reply.IsError) {
            await _log.AppendAsync(SessionEvent.Error(reply.ErrorKind, reply.ErrorMessage), new CancellationToken());
            Console.WriteLine("\n[error] " + (reply.ErrorKind != null ? reply.ErrorKind : "") + ": " + (reply.ErrorMessage != null ? reply.ErrorMessage : ""));
            return;
        }
        if (reply.Text != "") {
            await _log.AppendAsync(SessionEvent.Assistant(reply.Text), new CancellationToken());
            Console.WriteLine("\n[reply] " + reply.Text);
        }
    }
}
