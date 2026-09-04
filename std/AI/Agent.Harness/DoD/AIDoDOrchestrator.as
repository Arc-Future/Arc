// RFC 043：D0–D7 门编排骨架 — 委托 IAIDoDGateEvaluator；基座不写死 arc CLI。
namespace Arc.Agent.Harness;
using Arc;
using Arc.Agent;
using Arc.Collections;

/// <summary>DoD 编排器：按门顺序判定；失败不进入后续自动门。判定信号由 evaluator 提供。</summary>
public class AIDoDOrchestrator {
    private string _project;
    private IAIDoDGateEvaluator _evaluator;
    private int _fixRounds;
    private const int MaxFixRounds = 3;

    public AIDoDOrchestrator(string project, IAIDoDGateEvaluator evaluator) {
        if (evaluator == null) {
            throw new ArgumentException("evaluator is required");
        }
        _project = project != null && project != "" ? project : ".";
        _evaluator = evaluator;
        _fixRounds = 0;
    }

    public string Project {
        get { return _project; }
    }

    public int FixRounds {
        get { return _fixRounds; }
    }

    public void ResetFixRounds() {
        _fixRounds = 0;
    }

    public void RecordFixAttempt() {
        _fixRounds = _fixRounds + 1;
    }

    public bool FixBudgetExceeded {
        get { return _fixRounds >= MaxFixRounds; }
    }

    /// <summary>跑单门（委托领域 evaluator）。</summary>
    public async Task<AIDoDGateResult> RunGateAsync(
        AIDoDGateKind gate,
        AIRfc rfc,
        CancellationToken cancellationToken) {
        return await _evaluator.EvaluateAsync(gate, _project, rfc, cancellationToken);
    }

    /// <summary>
    /// 跑 D0→<paramref name="gate"/> 门链（gate ∈ D0..D3；超出按 D3）：先 D0 编译，过则依次
    /// D1、D2、D3，首个非 Passed 即停（返回该轮已跑结果）。D0–D3 是机器迭代面——
    /// 失败信号进入 <see cref="RunFixLoopAsync"/> 的结构化回喂。
    /// </summary>
    public async Task<List<AIDoDGateResult>> RunGatesAsync(
        AIDoDGateKind gate,
        AIRfc rfc,
        CancellationToken cancellationToken) {
        List<AIDoDGateResult> results = new List<AIDoDGateResult>();
        AIDoDGateResult d0 = await this.RunGateAsync(AIDoDGateKind.D0Compile, rfc, cancellationToken);
        results.Add(d0);
        if (d0.Status != AIDoDGateStatus.Passed || gate == AIDoDGateKind.D0Compile) {
            return results;
        }
        AIDoDGateResult d1 = await this.RunGateAsync(AIDoDGateKind.D1Semantics, rfc, cancellationToken);
        results.Add(d1);
        if (d1.Status != AIDoDGateStatus.Passed || gate == AIDoDGateKind.D1Semantics) {
            return results;
        }
        AIDoDGateResult d2 = await this.RunGateAsync(AIDoDGateKind.D2Contract, rfc, cancellationToken);
        results.Add(d2);
        if (d2.Status != AIDoDGateStatus.Passed || gate == AIDoDGateKind.D2Contract) {
            return results;
        }
        AIDoDGateResult d3 = await this.RunGateAsync(AIDoDGateKind.D3Behavior, rfc, cancellationToken);
        results.Add(d3);
        return results;
    }

    /// <summary>
    /// L2 自动迭代（RFC 043 场景 2.3 / 4.3，maxRounds=3 默认重载）：跑 D0–D3 门链 → Failed 时
    /// 结构化回喂（诊断文本 / 退出码 / --logger json 明细经 <see cref="AIDoDFixFeedback"/>）→
    /// <see cref="RecordFixAttempt"/> 计数 → 交给 <paramref name="fixProvider"/> 修复 → 重跑门。
    /// ≤maxRounds 轮内全绿 → Passed（携带轮数）；超限 → NeedsHuman（<see cref="AIDoDFixLoopResult.BudgetExceeded"/>）。
    /// 无修复提供者 → NeedsHuman（禁空转假循环）。Pending ≠ Passed（<see cref="AllPassed"/> 强制）。
    /// </summary>
    public async Task<AIDoDFixLoopResult> RunFixLoopAsync(
        AIDoDGateKind gate,
        AIRfc rfc,
        IAIFixRoundProvider? fixProvider,
        CancellationToken cancellationToken) {
        return await this.RunFixLoopAsync(gate, rfc, fixProvider, cancellationToken, AIDoDOrchestrator.MaxFixRounds);
    }

    /// <summary>L2 自动迭代（显式 maxRounds 版）。语义同上；超限判定 = 修复轮数达上限。</summary>
    public async Task<AIDoDFixLoopResult> RunFixLoopAsync(
        AIDoDGateKind gate,
        AIRfc rfc,
        IAIFixRoundProvider? fixProvider,
        CancellationToken cancellationToken,
        int maxRounds) {
        this.ResetFixRounds();
        if (maxRounds <= 0) {
            maxRounds = 1;
        }
        int round = 0;
        while (round <= maxRounds) {
            cancellationToken.ThrowIfCancellationRequested();
            List<AIDoDGateResult> results = await this.RunGatesAsync(gate, rfc, cancellationToken);
            if (AIDoDOrchestrator.AllPassed(results)) {
                return AIDoDFixLoopResult.Passed(gate, results, _fixRounds);
            }
            AIDoDFixFeedback feedback = AIDoDFixFeedback.FromFailing(results, _fixRounds + 1, _project);
            if (round >= maxRounds) {
                return AIDoDFixLoopResult.BudgetExceeded(gate, results, feedback, _fixRounds);
            }
            this.RecordFixAttempt();
            if (fixProvider == null) {
                return AIDoDFixLoopResult.NoProvider(gate, results, feedback);
            }
            await fixProvider.FixAsync(feedback, cancellationToken);
            round = round + 1;
        }
        return AIDoDFixLoopResult.BudgetExceeded(gate, new List<AIDoDGateResult>(), null, _fixRounds);
    }

    /// <summary>
    /// 跑自动门子集（D0 → D7）；未接线门由 evaluator 诚实 Pending；
    /// Pending ≠ Passed（见 AllPassed）。
    /// </summary>
    public async Task<List<AIDoDGateResult>> RunAutoGatesAsync(
        AIRfc rfc,
        CancellationToken cancellationToken) {
        List<AIDoDGateResult> results = new List<AIDoDGateResult>();
        AIDoDGateResult d0 = await this.RunGateAsync(AIDoDGateKind.D0Compile, rfc, cancellationToken);
        results.Add(d0);
        if (d0.Status != AIDoDGateStatus.Passed) {
            return results;
        }
        AIDoDGateResult d1 = await this.RunGateAsync(AIDoDGateKind.D1Semantics, rfc, cancellationToken);
        results.Add(d1);
        AIDoDGateResult d2 = await this.RunGateAsync(AIDoDGateKind.D2Contract, rfc, cancellationToken);
        results.Add(d2);
        AIDoDGateResult d3 = await this.RunGateAsync(AIDoDGateKind.D3Behavior, rfc, cancellationToken);
        results.Add(d3);
        if (d3.Status != AIDoDGateStatus.Passed) {
            return results;
        }
        AIDoDGateResult d4 = await this.RunGateAsync(AIDoDGateKind.D4DiffCoverage, rfc, cancellationToken);
        results.Add(d4);
        AIDoDGateResult d5 = await this.RunGateAsync(AIDoDGateKind.D5SelfReview, rfc, cancellationToken);
        results.Add(d5);
        AIDoDGateResult d6 = await this.RunGateAsync(AIDoDGateKind.D6AntiPattern, rfc, cancellationToken);
        results.Add(d6);
        AIDoDGateResult d7 = await this.RunGateAsync(AIDoDGateKind.D7HumanAccept, rfc, cancellationToken);
        results.Add(d7);
        return results;
    }

    /// <summary>Completed 可执行定义：全部 Passed；Pending ≠ Passed。</summary>
    public static bool AllPassed(List<AIDoDGateResult> results) {
        if (results == null || results.Count == 0) {
            return false;
        }
        int i = 0;
        int n = results.Count;
        while (i < n) {
            AIDoDGateResult r = results[i];
            if (r == null || r.Status != AIDoDGateStatus.Passed) {
                return false;
            }
            i = i + 1;
        }
        return true;
    }

    /// <summary>
    /// P3 汇总门唯一权威（parallel-subagents §3.6 / §4.4）：合并各子代理结果，先验全部终结且
    /// 必答小结；任一 Failed / 未完结 / 无小结 → 汇总门红（Pending ≠ Passed 由 <see cref="AllPassed"/>
    /// 强制）。全绿后对合并后总工作区跑完整 D0–D7（D4 对合并后总 diff 判定，非各子代理分片）。
    /// </summary>
    /// <param name="rfc">当前 AIRfc（合并后工作区判定基准）。</param>
    /// <param name="subAgents">全部子代理运行容器（含必答小结）。</param>
    /// <param name="cancellationToken">取消令牌。</param>
    public async Task<List<AIDoDGateResult>> RunAggregatedGatesAsync(
        AIRfc rfc,
        List<AISubAgentRun> subAgents,
        CancellationToken cancellationToken) {
        List<AIDoDGateResult> results = new List<AIDoDGateResult>();
        if (subAgents == null || subAgents.Count == 0) {
            results.Add(AIDoDGateResult.Fail(
                AIDoDGateKind.D3Behavior,
                "aggregated:no-subagents",
                "合并汇总须有子代理结果"));
            return results;
        }
        // D1：先验工作项持久状态源（跨会话可查）——任一 Failed / Cancelled 即红，失败信号
        // 不折叠成 Done；对齐状态源而非仅靠瞬态 run.Status 兜住。
        if (rfc != null && rfc.WorkItems != null) {
            int wi = 0;
            while (wi < rfc.WorkItems.Count) {
                AIRfcWorkItem item = rfc.WorkItems[wi];
                if (item != null
                    && (item.Status == AIRfcWorkItemStatus.Failed
                        || item.Status == AIRfcWorkItemStatus.Cancelled)) {
                    string state = AIRfcWorkItemStatusCodec.ToWireString(item.Status);
                    results.Add(AIDoDGateResult.Fail(
                        AIDoDGateKind.D3Behavior,
                        "aggregated:workitem-" + state + "-" + item.WorkItemId,
                        "工作项 " + item.WorkItemId + " 终态 " + state + "，汇总门红"));
                    return results;
                }
                wi = wi + 1;
            }
        }
        int i = 0;
        while (i < subAgents.Count) {
            AISubAgentRun run = subAgents[i];
            if (run == null) {
                results.Add(AIDoDGateResult.Fail(
                    AIDoDGateKind.D3Behavior,
                    "aggregated:null-run",
                    "子代理容器为 null"));
                return results;
            }
            if (run.Status != AITaskRunStatus.Completed || !run.HasSummary) {
                results.Add(AIDoDGateResult.Fail(
                    AIDoDGateKind.D3Behavior,
                    "aggregated:incomplete-" + run.WorkItemId,
                    "子代理 " + run.WorkItemId + " 未完结/无小结，汇总门红（Pending≠Passed）"));
                return results;
            }
            i = i + 1;
        }
        // 合并后总工作区跑完整 D0–D7（Pending ≠ Passed 由 AllPassed 强制）。
        return await this.RunAutoGatesAsync(rfc, cancellationToken);
    }
}
