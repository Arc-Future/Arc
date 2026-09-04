// RunOrchestrator —— /run 命令编排：一句话需求 → AIRfc → 计划树 → 单子代理 → 汇总门。
//
// 职责边界（分层：Repl 层）：只做「薄组装 + 交互展示」，不重复实现 PM/DoD——
// AIRfc 立项经 AIHarnessSession.SetRfc，任务图/并行协调器/汇总门均为框架既有面
// （AIRfcTaskGraph + AIParallelCoordinator + AIDoDOrchestrator.RunAggregatedGatesAsync）。
// 务实范围（实战差距审查 P1-3）：先接「单子代理」路径可跑（MaxConcurrentSubAgents=1、
// MaxStepsPerSubAgent 小值）；多子代理并行与子代理写工具 HITL 回环留后续。
namespace ArcAgent.Repl;
using Arc;
using Arc.Agent;
using Arc.Agent.Harness;
using ArcAgent.SessionLog;
using ArcAgent.Workspace;

/// <summary>/run 编排：一句话需求走完 AIRfc → 计划树 → 单子代理 → 汇总门全链路。</summary>
public class RunOrchestrator {
    private AIHost _host;
    private AISession _session;
    private AIHarnessSession _harness;
    private AgentWorkspace _workspace;
    private SessionEventLog _log;
    private AIPlanGate _planGate;
    private int _runCounter;

    public RunOrchestrator(AIHost host, AISession session, AIHarnessSession harness,
        AgentWorkspace workspace, SessionEventLog log, AIPlanGate planGate) {
        _host = host;
        _session = session;
        _harness = harness;
        _workspace = workspace;
        _log = log;
        _planGate = planGate;
        _runCounter = 0;
    }

    /// <summary>执行 /run <一句话需求>：立项 → 单工作项 → 计划树 → 单子代理 → 汇总门。</summary>
    public async Task RunAsync(string prompt, CancellationToken ct) {
        if (prompt == null || prompt.Trim() == "") {
            Console.WriteLine("usage: /run <一句话需求> — 一句话需求 → AIRfc → 计划树 → 子代理 → 汇总门");
            return;
        }
        // 1. 立项（AIRfc）：一句话需求 = Intention；Acceptance 取同句为最小可验收断言。
        _runCounter = _runCounter + 1;
        string rfcId = "RUN-" + _runCounter;
        AIAcceptanceSpec acceptance = new AIAcceptanceSpec();
        acceptance.Assertions = prompt;
        AIRfc? rfc = _harness.SetRfc(rfcId, new AIIntentionSpec(prompt), new AIDesignSpec(), acceptance);
        if (rfc == null) {
            Console.WriteLine("[run] 立项被拒（RfcSpec 租约冲突）");
            return;
        }
        Console.WriteLine("[run] 立项 " + rfc.RfcId + " v" + rfc.Revision);

        // 2. 绑定单工作项（单子代理路径：一需求一工作项）。
        AIRfcWorkItem? item = _harness.Runtime.BindWorkItem(rfcId, "W1", prompt, null, null);
        if (item == null) {
            Console.WriteLine("[run] 工作项绑定被拒（RfcSpec 租约冲突）");
            return;
        }

        // 3. 计划树（AIPlanTree 经 AIPlanGate.SetPlan 装配）+ 批准放行写入 + Plan 面回链 AIRfc。
        List<string> steps = new List<string>();
        steps.Add(prompt);
        AIPlan? plan = null;
        if (_planGate != null) {
            plan = _planGate.SetPlan(prompt, "single sub-agent run", steps, "arc build green");
        }
        if (plan == null) {
            Console.WriteLine("[run] 计划树装配失败（未挂载计划门闩或无步骤）");
            return;
        }
        _planGate.Approve();
        _harness.AttachPlan(plan);
        Console.WriteLine("[run] 计划树已装配并批准（" + plan.TotalSteps + " 步）");

        // 4. 任务图 + 并行协调器（单子代理：并行度 1、小步数有界收束）。
        AIRfcTaskGraph graph = new AIRfcTaskGraph(rfc.WorkItems);
        graph.DecisionSession = _session;
        Func<AIRfcWorkItem, AISession> factory = (wi: AIRfcWorkItem) => _host.CreateSession();
        AIParallelCoordinator coordinator = new AIParallelCoordinator(_host.Coordinator, _workspace.Sandbox, factory);
        coordinator.MaxConcurrentSubAgents = 1;
        coordinator.MaxStepsPerSubAgent = 4;
        coordinator.RfcRevision = rfc.Revision;
        coordinator.PrefixContext = rfc.ToContextBlock();
        List<AISubAgentRun> runs = await coordinator.RunAllAsync(graph, ct);
        if (runs.Count == 0) {
            Console.WriteLine("[run] 无子代理运行结果（收束为空）");
            return;
        }

        // 5. 汇总门：合并各子代理结果跑 D0–D7（Pending ≠ Passed）。
        List<AIDoDGateResult> gates = await _harness.DoD.RunAggregatedGatesAsync(rfc, runs, ct);
        bool green = AIDoDOrchestrator.AllPassed(gates);
        Console.WriteLine("[run] 汇总门 " + (green ? "绿（全 Passed）" : "红（Pending ≠ Passed）"));
        int i = 0;
        while (i < runs.Count) {
            AISubAgentRun run = runs[i];
            Console.WriteLine("[run] 子代理 " + run.WorkItemId + " " + this.RunStatusName(run.Status)
                + " (" + run.Steps + "/" + run.MaxSteps + " 步)");
            AIWorkSummary? summary = run.Summary;
            if (summary != null) {
                Console.WriteLine(summary.Format());
            }
            i = i + 1;
        }
        int j = 0;
        while (j < gates.Count) {
            AIDoDGateResult g = gates[j];
            if (g != null) {
                Console.WriteLine("[run] " + this.GateName(g.Gate) + " [" + this.GateStatusName(g.Status) + "]");
            }
            j = j + 1;
        }
        Console.WriteLine("[run] 完成：/dod 复核或 /summary 小结可继续");
    }

    private string RunStatusName(AITaskRunStatus status) {
        if (status == AITaskRunStatus.Completed) { return "Completed"; }
        if (status == AITaskRunStatus.Failed) { return "Failed"; }
        if (status == AITaskRunStatus.Cancelled) { return "Cancelled"; }
        if (status == AITaskRunStatus.Running) { return "Running"; }
        if (status == AITaskRunStatus.Paused) { return "Paused"; }
        return "Pending";
    }

    private string GateName(AIDoDGateKind gate) {
        if (gate == AIDoDGateKind.D0Compile) { return "D0 编译"; }
        if (gate == AIDoDGateKind.D1Semantics) { return "D1 语义"; }
        if (gate == AIDoDGateKind.D2Contract) { return "D2 契约"; }
        if (gate == AIDoDGateKind.D3Behavior) { return "D3 行为"; }
        if (gate == AIDoDGateKind.D4DiffCoverage) { return "D4 diff 覆盖"; }
        if (gate == AIDoDGateKind.D5SelfReview) { return "D5 自审"; }
        if (gate == AIDoDGateKind.D6AntiPattern) { return "D6 反模式"; }
        return "D7 人验收";
    }

    private string GateStatusName(AIDoDGateStatus status) {
        if (status == AIDoDGateStatus.Passed) { return "Passed"; }
        if (status == AIDoDGateStatus.Failed) { return "Failed"; }
        if (status == AIDoDGateStatus.NeedsHuman) { return "NeedsHuman"; }
        return "Pending";
    }
}
