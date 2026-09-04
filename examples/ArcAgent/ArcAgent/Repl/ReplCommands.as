// ReplCommands —— 斜杠命令路由（/help /plan /approve /reject /memory /remember /forget /usage
// /clear /sessions /resume /task）。
//
// 职责边界（分层：Repl 层）：只负责「识别并执行斜杠命令」；命中返回 true（已处理），
// 未命中返回 false（视为模型输入，交给 ReplSession 走任务回合）。不含 REPL 循环与回合调度。
// 计划门闩（RFC 038 M8.2）：/plan /approve /reject 统一走框架 AIPlanGate（审批状态机 +
// 生命周期事件回调）；会话事件日志经 AIPlanApprovalHandler 订阅在事件回调中落盘，命令层
// 不再手写门闩/日志（与工具层零门槛对称）。
namespace ArcAgent.Repl;
using Arc;
using Arc.Agent;
using Arc.Agent.Harness;
using ArcAgent.Context;
using ArcAgent.SessionLog;
using ArcAgent.Workspace;

/// <summary>斜杠命令处理：命中返回 true（已处理），否则返回 false（作为模型输入）。</summary>
public class ReplCommands {
    private AIHost _host;
    private AISession _session;
    private AgentContext _context;
    private SessionEventLog _log;
    private AIPlanGate _planGate;
    private AgentWorkspace _workspace;
    private DirectionLoop _direction;
    private AIHarnessSession _harness;
    private RunOrchestrator _run;

    public ReplCommands(AIHost host, AISession session, AgentContext context, SessionEventLog log,
        AgentWorkspace workspace, AIHarnessSession harness) {
        _host = host;
        _session = session;
        _context = context;
        _log = log;
        _workspace = workspace;
        _harness = harness;
        _planGate = host != null ? host.PlanGate : null;
        if (_planGate != null) {
            this.SubscribePlanEvents();
        }
        // 方向环（H-4）：AIRfc 立项/升版/拒绝/小结/绿点回滚/DoD（D5 槽位 + D7 人验收；D0–D3
        // 失败进入 L2 自动迭代——ReplFixRoundProvider 以模型回合驱动修复）。
        _direction = new DirectionLoop(harness, workspace, log, _planGate, session);
        // /run 编排（实战差距审查 P1-3）：一句话需求 → AIRfc → 计划树 → 单子代理 → 汇总门。
        _run = new RunOrchestrator(host, session, harness, workspace, log, _planGate);
    }

    /// <summary>订阅计划生命周期事件 → 会话事件日志（模型/人类驱动的计划行为全部可审计）。</summary>
    private void SubscribePlanEvents() {
        AIPlanApprovalHandler events = new AIPlanApprovalHandler();
        events.OnPlanCreated = (plan: AIPlan) => {
            _log.AppendAsync(SessionEvent.Approval("plan", "created", "model created plan v" + plan.Revision), new CancellationToken());
        };
        events.OnPlanRevised = (plan: AIPlan) => {
            _log.AppendAsync(SessionEvent.Approval("plan", "revised", "model revised plan to v" + plan.Revision), new CancellationToken());
        };
        events.OnPlanApproved = (plan: AIPlan) => {
            _log.AppendAsync(SessionEvent.Approval("plan", "approved", "human approved plan"), new CancellationToken());
        };
        events.OnPlanRejected = (plan: AIPlan) => {
            _log.AppendAsync(SessionEvent.Approval("plan", "rejected", "human rejected plan"), new CancellationToken());
        };
        events.OnPlanVerifying = (plan: AIPlan) => {
            _log.AppendAsync(SessionEvent.Approval("plan", "verifying", "all steps done — awaiting DoD verdict (D0-D7)"), new CancellationToken());
        };
        events.OnPlanCompleted = (plan: AIPlan) => {
            _log.AppendAsync(SessionEvent.Approval("plan", "completed", "DoD D0-D7 passed — plan completed"), new CancellationToken());
        };
        _planGate.SetEvents(events);
    }

    /// <summary>尝试处理斜杠命令；未命中返回 false。</summary>
    public async Task<bool> TryHandleAsync(string trimmed) {
        // 方向环（H-4）：/rfc /revise /reject <reason> /summary /checkpoint /rollback /dod /save
        // + B1 /conflict（L2 冲突列表 + 人 CCB 裁决）。
        // /reject 精确无参保持计划门闩拒绝（兼容既有行为）；带 reason 走 AIRfc 方向拒绝。
        if (trimmed == "/rfc" || trimmed.StartsWith("/rfc ")
            || trimmed.StartsWith("/revise")
            || trimmed.StartsWith("/reject ")
            || trimmed == "/summary"
            || trimmed == "/checkpoint" || trimmed.StartsWith("/checkpoint ")
            || trimmed == "/rollback" || trimmed.StartsWith("/rollback ")
            || trimmed == "/dod" || trimmed.StartsWith("/dod ")
            || trimmed == "/save"
            || trimmed == "/conflict" || trimmed.StartsWith("/conflict ")) {
            await _direction.TryHandleAsync(trimmed);
            return true;
        }
        if (trimmed == "/run") {
            Console.WriteLine("usage: /run <一句话需求> — 一句话需求 → AIRfc → 计划树 → 子代理 → 汇总门");
            return true;
        }
        if (trimmed.StartsWith("/run ")) {
            await _run.RunAsync(trimmed.Substring("/run ".Length).Trim(), new CancellationToken());
            return true;
        }
        if (trimmed == "/help") {
            this.ShowHelp();
            return true;
        }
        if (trimmed == "/plan") {
            this.ShowPlan();
            return true;
        }
        if (trimmed == "/approve") {
            await this.ApprovePlanAsync();
            return true;
        }
        if (trimmed == "/reject") {
            await this.RejectPlanAsync();
            return true;
        }
        if (trimmed == "/memory") {
            this.ShowMemory();
            return true;
        }
        if (trimmed.StartsWith("/remember ")) {
            await this.RememberAsync(trimmed.Substring("/remember ".Length));
            return true;
        }
        if (trimmed.StartsWith("/forget ")) {
            await this.ForgetAsync(trimmed.Substring("/forget ".Length));
            return true;
        }
        if (trimmed == "/usage") {
            this.ShowUsage();
            return true;
        }
        if (trimmed == "/clear") {
            _session.Clear();
            Console.WriteLine("conversation cleared");
            return true;
        }
        if (trimmed == "/sessions") {
            await this.ShowSessionsAsync();
            return true;
        }
        if (trimmed.StartsWith("/resume")) {
            await this.ResumeAsync(trimmed.Substring("/resume".Length).Trim());
            return true;
        }
        return false;
    }

    private void ShowHelp() {
        Console.WriteLine("  /help                          show commands");
        Console.WriteLine("  /plan                          show current task plan");
        Console.WriteLine("  /approve                       approve the pending plan (unlocks write tools)");
        Console.WriteLine("  /reject                        reject the pending plan (model must revise)");
        Console.WriteLine("  /memory                        list memory pages");
        Console.WriteLine("  /remember <path> <text>        store a memory page (e.g. /remember project/build \"cargo test -p x\")");
        Console.WriteLine("  /forget <path>                 remove a memory page");
        Console.WriteLine("  /usage                         show token usage");
        Console.WriteLine("  /clear                         clear conversation");
        Console.WriteLine("  /sessions                      list past sessions");
        Console.WriteLine("  /resume [id]                   resume a session (default = newest)");
        Console.WriteLine("  /task <prompt>                 start an autonomous multi-turn task run");
        Console.WriteLine("  /run <一句话需求>              一句话需求 → AIRfc → 计划树 → 子代理 → 汇总门（实战 P1-3）");
        Console.WriteLine("  方向环（H-4，RFC 043）：");
        Console.WriteLine("  /rfc <意图>                    AIRfc 立项（Revision 1，airfc:created；Design/Acceptance 空缺 → 澄清向导追问）");
        Console.WriteLine("  /revise <理由> [--intention=][--design=][--acceptance=]   升版纠偏（airfc:revised）");
        Console.WriteLine("  /reject <reason>               AIRfc 方向拒绝（Active → Rejected，airfc:rejected）");
        Console.WriteLine("  /summary                       工作单元小结（五字段决策面 + 偏差判定）");
        Console.WriteLine("  /checkpoint [label]            绿点快照（checkpoint:green）");
        Console.WriteLine("  /rollback [--cp=<绿点id>] [reason]   回滚到指定绿点（缺省最近；AIRfc/Plan 联动，checkpoint:rollback）");
        Console.WriteLine("  /dod                           跑 DoD 全门（D0–D7）+ D5 证明槽位 + D7 一次人验收");
        Console.WriteLine("  /dod d5 [<序号> <证明>]        查看/填 D5 自审证明（测试/文件）；无证明项标红");
        Console.WriteLine("  /save                          持久化 AIRfc 聚合根（target/scratch/arcagent-state/airfc.json）");
        Console.WriteLine("  /resume 自动恢复 AIRfc         续跑时经 /resume 重建聚合根（非 transcript 重放冒充）");
        Console.WriteLine("  /conflict                      列出 Open 冲突（L2 Spec 矛盾；方向/来源/evidence）");
        Console.WriteLine("  /conflict <id>                 查看冲突详情（含双方 acceptance 快照）");
        Console.WriteLine("  /conflict resolve <id> [--after] [--by=<CCB>] <reason>   人 CCB 裁决 → 新 Revision 基线 + airfc:resolved（机器不可自动选胜者）");
        Console.WriteLine("  /conflict reject <id> [--by=<CCB>] <reason>              拒绝冲突（AIRfc → Rejected）");
        Console.WriteLine("  exit                           quit and persist memory");
    }

    /// <summary>展示当前任务计划（AIPlanGate 持有，模型同源可见）。</summary>
    private void ShowPlan() {
        AIPlan plan = _planGate != null ? _planGate.GetPlan() : null;
        if (plan == null) {
            Console.WriteLine("(no plan yet)");
            return;
        }
        Console.WriteLine(plan.ToMarkdown());
    }

    /// <summary>批准当前计划：状态机放行写入型工具（OnPlanApproved 事件落会话日志）。</summary>
    private async Task<void> ApprovePlanAsync() {
        if (_planGate == null) {
            Console.WriteLine("plan gate not enabled");
            return;
        }
        Console.WriteLine(_planGate.Approve());
    }

    /// <summary>拒绝当前计划：置 Rejected 迫模型出修订版（OnPlanRejected 事件落会话日志）。</summary>
    private async Task<void> RejectPlanAsync() {
        if (_planGate == null) {
            Console.WriteLine("plan gate not enabled");
            return;
        }
        Console.WriteLine(_planGate.Reject());
    }

    private void ShowMemory() {
        Console.WriteLine("memory file: " + _context.MemoryFile);
        List<string> paths = _context.KnowledgePaths(_host.Wiki);
        if (paths.Count == 0) {
            Console.WriteLine("(empty)");
            return;
        }
        foreach (var path in paths) {
            Console.WriteLine("  - " + path);
        }
    }

    private async Task RememberAsync(string rest) {
        int sp = rest.IndexOf(" ");
        if (sp <= 0) {
            Console.WriteLine("usage: /remember <path> <text>");
            return;
        }
        string path = rest.Substring(0, sp).Trim();
        string text = rest.Substring(sp + 1).Trim();
        _host.Wiki.Upsert(path, text);
        bool saved = await _context.SaveWikiAsync(_host.Wiki, new CancellationToken());
        Console.WriteLine("stored '" + path + "' (" + (saved ? "saved" : "memory not persisted") + ")");
    }

    private async Task ForgetAsync(string path) {
        bool removed = _host.Wiki.Delete(path);
        bool saved = await _context.SaveWikiAsync(_host.Wiki, new CancellationToken());
        Console.WriteLine("forget '" + path + "': " + (removed ? "removed" : "not found") + (saved ? ", saved" : ", not persisted"));
    }

    private void ShowUsage() {
        AITokenUsage u = _session.TotalUsage;
        Console.WriteLine("  prompt=" + u.PromptTokens + " completion=" + u.CompletionTokens
            + " total=" + u.TotalTokens
            + " cacheRead=" + u.CacheReadTokens + " cacheWrite=" + u.CacheCreationTokens);
    }

    private async Task ShowSessionsAsync() {
        List<SessionInfo> sessions = await _log.ListAsync(new CancellationToken());
        if (sessions.Count == 0) {
            Console.WriteLine("(no saved sessions)");
            return;
        }
        Console.WriteLine("sessions (" + sessions.Count + "):");
        foreach (var info in sessions) {
            long ts = Convert.ToInt64(info.Created);
            DateTime dt = new DateTime(ts * 10000);
            string date = dt.ToString("yyyy-MM-dd HH:mm");
            Console.WriteLine("  " + info.Id + "  " + info.Title + "  (" + info.EventCount + " events)  " + date);
        }
    }

    private async Task ResumeAsync(string id) {
        string idToResume = id;
        if (idToResume == "") {
            List<SessionInfo> list = await _log.ListAsync(new CancellationToken());
            if (list.Count == 0) {
                Console.WriteLine("no sessions to resume");
                return;
            }
            idToResume = list[0].Id;
            Console.WriteLine("resuming newest: " + idToResume);
        }
        AISessionSnapshot snap = await _log.BuildSnapshotAsync(idToResume, new CancellationToken());
        _session.Restore(snap);
        await _log.ResumeAsync(idToResume, new CancellationToken());
        Console.WriteLine("session " + idToResume + " resumed: " + snap.Transcript.Count + " messages");
        // 2.4 续跑前提：重建 AIRfc 聚合根（非 transcript 重放冒充；无状态文件 → 优雅降级）。
        await _direction.RestoreStateAsync();
    }
}
