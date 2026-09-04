// RFC 043 P3（parallel-subagents §3.3 / §4）+ A1 + A2（subagent-management）：并行子代理协调器。
// 按任务图 Ready 有界扇出、受 MaxConcurrentSubAgents 节流、汇合入账；每工作项一个独立
// AISession + 任务专属上下文块 + 只读共享 AIRfc 快照（RfcRevision）。
// **只消费 Arc.Agent.AICoordinator**（写文件走 ToolPath 租约、AIRfc 写回走 RfcSpec 租约），
// 不发明第二套锁/队列/事件。
//
// A1（2026-08-15）：**租约惰性化 + reconcile 循环化**——① 不再派发即预取整波 Scope 租约，
// 改为惰性租约门（AISubAgentLeaseGate → sandbox 调度层）首次真实写前逐个 Acquire，同波
// 工作项不再因预取互相误伤（假冲突消除）；② RunAllAsync 从「波次 await 到完结」改为
// reconcile 循环：每 tick 检查在飞状态（Running/Failed/Dead 心跳）、逐回合 RunStepAsync
// 推进、派发新 Ready、释放 Failed/Dead 租约、检查预算；③ 终结同 tick 即时 ReleaseSession，
// 不再延迟到 RunAllAsync 尾部。
//
// A2（2026-08-15）：**生命周期状态机 + 撤单收束**——① DispatchAsync 即 run.Spawn()（精化
// 状态 Spawned）；② 取消令牌被吞修复：ct 取消/撤单中断 → 子代理转 Cancelled + 必答小结
// （不再被 reply.IsError 吞成 Failed、不再被 Complete() 无条件覆盖），汇总门按未完结红
// （Pending≠Passed）；③ CancelPendingAsync(rfcId)：取消未启动（Open/Blocked → Cancelled，
// 不进下一波）+ 中断在飞（联动 CTS + AITaskRun.Cancel）+ 即时释放该 Rfc 全部租约 + 回收
// 容器（Dead）；④ 决策事件 airfc:cancelled / subagent:interrupt 入决策轨迹。
//
// A3（2026-08-15）：**决策广播 + 重对齐 + 租约重验**——① 新类型 AISubAgentMessage /
// AISubAgentDecision（§8 载荷）；② 旁路注入 EnqueueMessageAsync（soft：邮箱 PendingMessages，
// 下回合拼入 prompt delta，不动前缀稳定块）；③ 决策广播/定向 PendingSyncDecisionAsync /
// SyncDecisionAsync（hard：revision-changed → 全部在飞检查点 + 重建 ContextBlock 到新
// Revision + 租约重验（Scope 变冲突 → Failed + 小结；不变 → 继续）；work-item-rescope →
// 定向重取 Scope 租约；wrap-up → 旁路注入收束部分成果）；④ 决策事件 subagent:sync 入轨迹；
// ⑤ /revise 接线：AIHarnessSession.ReviseRfc 成功后经 RfcRevisionChanged 钩子广播。
//
// A4（2026-08-16）：**动态派发 + 弹性并行度 + 预算强制收束**——① graph.AttachItem 运行中增项 +
// SpawnAsync 派发原语（派发前校验在飞数 < MaxConcurrentSubAgents，惰性取 Scope ToolPath 租约）；
// ② SetParallelism 弹性（上调立即多派、下调停止新派在飞跑完）；③ TotalBudget 从「只算」到
// 「强制」——reconcile 每 tick 检查 Σ子代理回合用量 > TotalBudget → 收束梯：停止新派发 →
// wrap-up 旁路注入（宽限 1 回合）→ 宽限后强制中断 → Failed(BudgetExceeded) + 小结 + 升级人。
//
// A5（2026-08-16）：**成本核算 / 观测**——① TotalUsage 聚合各 run 的 AITokenUsage（token/轮次
// 统计）随终结入账；② subagent:usage 决策事件入各 run 会话轨迹（单轨）；③ 暴露成本观测面
// TotalUsage / TotalTurns / TotalBudgetExceeded / InFlightCount。
//
// 并发模型：单线程宿主下为**逻辑并行 = async 交错**（每工作项独立 Session，Async 交错跑），
// 不宣称多线程并发驱动同一 AIHost（B5）。演进方向：本类型逐步收敛为子代理管理面
// AISubAgentManager（方案 A；当前保留类名，禁双轨）。
namespace Arc.Agent.Harness;
using Arc;
using Arc.Agent;
using Arc.Collections;

/// <summary>
/// 并行子代理协调器：只消费 <see cref="AICoordinator"/>（冲突织物）做写面仲裁，不发明第二套锁。
/// A1 起**惰性取租约**：首次真实写前由 <see cref="AISubAgentLeaseGate"/> 取 ToolPath 租约；
/// 后到拒绝（首次写时路径被占，Acquired=false）→ 该子代理 Failed + 必答小结上报
/// （不静默降级、不自旋等待）。终结同 tick 即时释放租约。A2 起支持撤单收束
/// （<see cref="CancelPendingAsync"/>）+ 生命周期精化状态（<see cref="AISubAgentState"/>）。
/// A3 起支持决策广播 / 定向（<see cref="PendingSyncDecisionAsync"/> / <see cref="SyncDecisionAsync"/>：
/// revision-changed 重对齐 + 租约重验，work-item-rescope 定向重取，wrap-up 旁路注入）+ 旁路
/// 注入（<see cref="EnqueueMessageAsync"/>，soft 邮箱）。A4 起支持动态派发（<see cref="SpawnAsync"/>）
/// + 弹性并行度（<see cref="SetParallelism"/>）+ 预算强制收束梯（wrap-up → 中断 → Failed(BudgetExceeded)）；
/// A5 起暴露成本观测面（<see cref="TotalUsage"/> / <see cref="TotalTurns"/> /
/// <see cref="TotalBudgetExceeded"/> / <see cref="InFlightCount"/>）。
/// </summary>
public class AIParallelCoordinator {
    private AICoordinator _coordinator;
    private AIWorkspace _workspace;
    private Func<AIRfcWorkItem, AISession> _sessionFactory;
    private int _maxConcurrentSubAgents;
    private int _rfcRevision;
    private string _prefixContext;
    private int _subAgentBudget;
    private int _aggregateBudget;
    private int _maxStepsPerSubAgent;
    private long _heartbeatTimeoutMs;
    // 可选：AIRfc 写回主会话的 RfcSpec 租约上下文（holderId + rfcId）。未挂载 = 不参与跨会话仲裁。
    private string _rfcLeaseRfcId;
    private string _rfcLeaseHolderId;
    // A2 撤单收束运行态：当前活动任务图 + 在飞子代理 + 联动 CTS（CancelPendingAsync 操作对象）。
    private AIRfcTaskGraph? _activeGraph;
    private List<AISubAgentRun>? _inflightRuns;
    private CancellationTokenSource? _cancelCts;
    private bool _cancelRequested;
    // A3 决策同步运行态：待决广播决策（PendingSyncDecisionAsync 投递；reconcile 同 tick 消费）。
    private AISubAgentDecision? _pendingDecision;
    private bool _decisionPending;
    // A5 成本观测：聚合各 run 的 token 用量 + 轮次 + 预算超限标记（RunAllAsync 起始重置，终结入账）。
    private AITokenUsage _totalUsage;
    private int _totalTurns;
    private bool _totalBudgetExceeded;
    // A4 预算收束梯运行态（RunAllAsync 起始重置）：wrap-up 已旁路注入标记（宽限 1 回合后强制中断）。
    private bool _wrapupInjected;

    public AIParallelCoordinator(
        AICoordinator coordinator,
        AIWorkspace workspace,
        Func<AIRfcWorkItem, AISession> sessionFactory) {
        if (coordinator == null) {
            throw new ArgumentException("coordinator is required");
        }
        if (workspace == null) {
            throw new ArgumentException("workspace is required");
        }
        _coordinator = coordinator;
        _workspace = workspace;
        _sessionFactory = sessionFactory;
        _maxConcurrentSubAgents = 3;
        _rfcRevision = 0;
        _prefixContext = "";
        _subAgentBudget = 0;
        _aggregateBudget = 0;
        _maxStepsPerSubAgent = 8;
        _heartbeatTimeoutMs = 60000;
        _rfcLeaseRfcId = "";
        _rfcLeaseHolderId = "";
        _activeGraph = null;
        _inflightRuns = null;
        _cancelCts = null;
        _cancelRequested = false;
        _pendingDecision = null;
        _decisionPending = false;
        _totalUsage = new AITokenUsage();
        _totalTurns = 0;
        _totalBudgetExceeded = false;
        _wrapupInjected = false;
    }

    /// <summary>并行度上限（默认 3；clamp ≥ 1）。</summary>
    public int MaxConcurrentSubAgents {
        get { return _maxConcurrentSubAgents; }
        set {
            int v = value > 0 ? value : 1;
            _maxConcurrentSubAgents = v;
        }
    }

    /// <summary>
    /// A4 弹性并行度：动态调整并行度上限（clamp ≥ 1）。上调 → reconcile 下一 tick 立即多派；
    /// 下调 → 停止新派、在飞跑完（禁为降并行度硬杀在飞子代理）。reconcile 循环每 tick 读
    /// <see cref="MaxConcurrentSubAgents"/>，运行中调用即下一 tick 生效。
    /// </summary>
    public void SetParallelism(int max) {
        int v = max > 0 ? max : 1;
        _maxConcurrentSubAgents = v;
    }

    /// <summary>当前在飞子代理数（成本/弹性观测面；未启动 RunAllAsync 时 = 0）。</summary>
    public int InFlightCount {
        get { return _inflightRuns != null ? _inflightRuns.Count : 0; }
    }

    /// <summary>启动时 AIRfc 只读快照的 Revision（每个子代理运行承载）。</summary>
    public int RfcRevision {
        get { return _rfcRevision; }
        set { _rfcRevision = value; }
    }

    /// <summary>全局上下文块前缀（共用需求/设计/验收摘要，跨子代理复用 KV cache）。</summary>
    public string PrefixContext {
        get { return _prefixContext; }
        set { _prefixContext = value != null ? value : ""; }
    }

    /// <summary>单子代理预算（TotalBudget 累加项）。</summary>
    public int SubAgentBudget {
        get { return _subAgentBudget; }
        set { _subAgentBudget = value; }
    }

    /// <summary>汇总门开销预算（TotalBudget 累加项）。</summary>
    public int AggregateBudget {
        get { return _aggregateBudget; }
        set { _aggregateBudget = value; }
    }

    /// <summary>单子代理有界回合上限（AISubAgentRun maxSteps）。</summary>
    public int MaxStepsPerSubAgent {
        get { return _maxStepsPerSubAgent; }
        set {
            int v = value > 0 ? value : 1;
            _maxStepsPerSubAgent = v;
        }
    }

    /// <summary>
    /// 心跳超时阈值（毫秒；0 = 从不判定 Dead）。reconcile 循环每 tick 对在飞 Running 子代理
    /// 检查心跳（IsStale）；超时 → 该子代理 Dead → 同 tick 释放其租约（被占路径可被后续取得）。
    /// </summary>
    public long HeartbeatTimeoutMs {
        get { return _heartbeatTimeoutMs; }
        set { _heartbeatTimeoutMs = value > 0 ? value : 0; }
    }

    /// <summary>
    /// 总成本预算 = Σ 各子代理预算 + 汇总门开销（§4.5）。graph 提供工作项数。
    /// </summary>
    public int TotalBudget(AIRfcTaskGraph graph) {
        int count = 0;
        if (graph != null && graph.Items != null) {
            count = graph.Items.Count;
        }
        return count * _subAgentBudget + _aggregateBudget;
    }

    /// <summary>
    /// A5 成本核算：聚合各 run 的 token 用量（含缓存命中/写入）。随 run 终结入账
    /// （<see cref="FinalizeRun"/>）；每次 <see cref="RunAllAsync"/> 起始重置。返回拷贝，
    /// 外部改写不影响内部累积。
    /// </summary>
    public AITokenUsage TotalUsage {
        get {
            AITokenUsage copy = new AITokenUsage();
            copy.PromptTokens = _totalUsage.PromptTokens;
            copy.CompletionTokens = _totalUsage.CompletionTokens;
            copy.TotalTokens = _totalUsage.TotalTokens;
            copy.CacheReadTokens = _totalUsage.CacheReadTokens;
            copy.CacheCreationTokens = _totalUsage.CacheCreationTokens;
            return copy;
        }
    }

    /// <summary>A5 成本核算：聚合各 run 的总轮次（steps）统计（随 run 终结入账）。</summary>
    public int TotalTurns {
        get { return _totalTurns; }
    }

    /// <summary>A4 预算强制收束：本批是否已触发 TotalBudget 超限（reconcile 检测到即置位）。</summary>
    public bool TotalBudgetExceeded {
        get { return _totalBudgetExceeded; }
    }

    /// <summary>
    /// 挂载 RfcSpec 租约上下文：AIRfc 写回主会话经 <see cref="AILeaseKind.RfcSpec"/> 租约仲裁
    /// （holderId = 主会话 id）。未挂载 = 进程内登记（不参与跨会话仲裁）。
    /// </summary>
    public void AttachRfcLease(string rfcId, string holderId) {
        _rfcLeaseRfcId = rfcId != null ? rfcId : "";
        _rfcLeaseHolderId = holderId != null ? holderId : "";
    }

    /// <summary>
    /// 为单个就绪工作项派发子代理容器：经 <paramref name="sessionFactory"/> 创建独立 AISession，
    /// 组装任务专属上下文块（全局前缀稳定 + Scope/DependsOn 摘要），并装配惰性 ToolPath 租约门
    /// （<see cref="AISubAgentLeaseGate"/> → sandbox 调度层）。**不再派发即预取整波 Scope 租约**——
    /// 首次真实写前由门逐个 Acquire；同波工作项不再因预取互相误伤（A1 假冲突消除）。真冲突
    /// （首次写时路径被其它会话持有）→ 后到拒绝 → 该子代理 Failed + 必答小结上报（不排队、不自旋）。
    /// A2：创建即 <see cref="AISubAgentRun.Spawn"/>（精化状态 Spawned）。
    /// </summary>
    public async Task<AISubAgentRun> DispatchAsync(
        AIRfcWorkItem workItem,
        string contextBlock,
        CancellationToken cancellationToken) {
        AISession session = _sessionFactory(workItem);
        AISubAgentRun run = new AISubAgentRun(session, _maxStepsPerSubAgent);
        run.WorkItemId = workItem.WorkItemId;
        run.RfcRevision = _rfcRevision;
        run.ContextBlock = this.BuildTaskContext(workItem, contextBlock);
        run.Spawn();

        // A1 租约惰性化：门以工作项 Scope 为声明写面，sandbox 调度层首次真实写前经门取租约。
        // 注：经局部变量装配 sandbox 门（属性链上直接 set 属性 codegen 接收者类型错位）。
        AISubAgentLeaseGate gate = new AISubAgentLeaseGate(
            _coordinator, _workspace, session.SessionId, workItem.Scope);
        run.SetLeaseGate(gate);
        AIToolSandbox? sandbox = session.Sandbox;
        if (sandbox != null) {
            sandbox.LeaseGate = gate;
        }
        return run;
    }

    /// <summary>
    /// A4 动态派发原语：为单个就绪工作项派发子代理容器（Pending→Spawned）。派发前校验在飞数
    /// &lt; <see cref="MaxConcurrentSubAgents"/>（超限 → null，排下一波）；经 sessionFactory 建
    /// 独立 AISession 并装配惰性 ToolPath 租约门（首次真实写前取租约，不派发即预取整波）。不负责
    /// <see cref="AIRfcTaskGraph.MarkInProgress"/>（就绪面占用由调用方按图语义执行）。reconcile 循环
    /// 与外部「运行中增项」共用本原语。null = 超限 / 空工作项（排下一波）。
    /// </summary>
    public async Task<AISubAgentRun> SpawnAsync(AIRfcWorkItem item, CancellationToken cancellationToken) {
        if (item == null) {
            return null;
        }
        if (_inflightRuns != null && _inflightRuns.Count >= _maxConcurrentSubAgents) {
            return null;
        }
        return await this.DispatchAsync(item, item.Title, cancellationToken);
    }

    /// <summary>
    /// 有界收束跑完任务图（A1 reconcile 循环，替代旧「波次 await 到完结」）：每 tick ① 派发
    /// 新 Ready（受 <see cref="MaxConcurrentSubAgents"/> 节流）→ ② 在飞 Running 子代理各推进
    /// 一回合（<see cref="AITaskRun.RunStepAsync"/>；心跳/时长熔断 → Dead/Failed）→ ③ 终结
    /// （Failed/Completed/Cancelled）同 tick 入账 + <see cref="AICoordinator.ReleaseSession"/> 即时
    /// 释放租约 → ④ 检查预算（<see cref="TotalBudget"/> 超限 → A4 收束梯：停止新派发 → wrap-up
    /// 旁路注入（宽限 1 回合）→ 强制中断 → Failed(BudgetExceeded) + 小结 + 升级人）。后到拒绝的
    /// 工作项（首写冲突）经惰性租约门标 Failed，小结已上报。
    /// A2：ct 取消/撤单中断 → 子代理转 Cancelled + 小结（取消不再被吞），终结经
    /// <see cref="AIRfcTaskGraph.MarkCancelled"/> 入账；撤单由 <see cref="CancelPendingAsync"/> 驱动。
    /// A3：每 tick ②.5 消费待决决策（<see cref="PendingSyncDecisionAsync"/> 广播 /
    /// <see cref="SyncDecisionAsync"/> 定向）——revision-changed 重对齐 + 租约重验 /
    /// work-item-rescope 定向重取 / wrap-up 旁路注入；③ 回合前折叠旁路注入邮箱消息
    /// （<see cref="AISubAgentRun.DrainMessages"/>）进 prompt delta。
    /// A4/A5：运行中经 <see cref="AIRfcTaskGraph.AttachItem"/> 增项自动进就绪面派发；终结时聚合
    /// token/轮次入 <see cref="TotalUsage"/> 并写 subagent:usage 决策事件。
    /// </summary>
    public async Task<List<AISubAgentRun>> RunAllAsync(
        AIRfcTaskGraph graph,
        CancellationToken cancellationToken) {
        List<AISubAgentRun> runs = new List<AISubAgentRun>();
        if (graph == null) {
            return runs;
        }
        // A2 撤单收束运行态：挂活动图 + 在飞列表 + 联动 CTS（外部 CancelPendingAsync 的操作面）。
        _activeGraph = graph;
        _inflightRuns = new List<AISubAgentRun>();
        _cancelRequested = false;
        _cancelCts = new CancellationTokenSource();
        CancellationToken linkedToken = _cancelCts.Token;
        // A5 成本核算：每次 RunAllAsync 起始重置聚合用量（单批口径，随终结重新入账）。
        this.ResetUsage();
        // AIRfc 写回主会话走 RfcSpec 租约：挂载后先取租约（后到拒绝 → 收束返回空，升级人）。
        if (this.HasRfcLease()) {
            AILeaseKey rfcKey = AILeaseKey.RfcSpec(_rfcLeaseRfcId);
            AIResourceGrant grant = _coordinator.Acquire(_rfcLeaseHolderId, rfcKey);
            if (grant == null || !grant.Acquired || !_coordinator.CommitRfcSpec(_rfcLeaseHolderId, rfcKey)) {
                this.SettleRunState();
                return runs;
            }
        }
        long budget = this.TotalBudget(graph);
        long consumedSteps = 0;
        // A4 预算收束梯运行态：以字段承载（禁局部 bool 跨 await 读——规避编译器 async 状态机
        // 局部变量跨 await 丢失的已知缺陷）；在飞列表沿用局部 <c>inFlight</c>（既有 A1 惯用法）。
        _wrapupInjected = false;
        List<AISubAgentRun> inFlight = _inflightRuns;
        while (graph.HasRemaining) {
            // A2 外部 ct 取消联动：每 tick 探测调用方取消令牌 → 转 Cancelled（取消不再被吞）。
            if (!_cancelRequested && cancellationToken != null && cancellationToken.IsCancellationRequested) {
                _cancelRequested = true;
                _cancelCts.Cancel();
            }
            bool budgetExhausted = budget > 0 && consumedSteps >= budget;
            // A4 ① 预算收束：停止新派发（在飞继续 wrap-up 收束）；否则派发新 Ready 工作项
            // （受 MaxConcurrentSubAgents 节流）。
            if (!budgetExhausted) {
                List<AIRfcWorkItem> ready = graph.Ready();
                int ri = 0;
                while (ri < ready.Count && inFlight.Count < _maxConcurrentSubAgents) {
                    AIRfcWorkItem item = ready[ri];
                    if (graph.MarkInProgress(item.WorkItemId)) {
                        AISubAgentRun run = await this.DispatchAsync(item, item.Title, linkedToken);
                        runs.Add(run);
                        run.NextPrompt = run.ContextBlock != null && run.ContextBlock != ""
                            ? run.ContextBlock : run.WorkItemId;
                        inFlight.Add(run);
                    }
                    ri = ri + 1;
                }
            }
            // ② 无在飞且仍有剩余 → 依赖未解（环/外部阻塞）→ 收束（剩余保持未启动，升级人）。
            if (inFlight.Count == 0) {
                break;
            }
            // ②.5 A3 决策同步：广播/定向待决决策同 tick 消费——revision-changed →
            // 全部在飞检查点 + 重建 ContextBlock 到新 Revision + 租约重验（冲突 → Failed）；
            // work-item-rescope → 定向重取 Scope 租约；wrap-up → 旁路注入「收束部分成果」。
            this.ProcessPendingDecisions(graph, inFlight);
            // ③ 每 tick 把在飞子代理各推进一回合（原子步进；心跳 Dead/时长超限 → 同 tick 收束）。
            int si = 0;
            while (si < inFlight.Count) {
                AISubAgentRun run = inFlight[si];
                if (run.State == AISubAgentState.Interrupted) {
                    // A3 决策中断态：决策已在 ②.5 同 tick 处理（重对齐恢复或 Failed/终态）；
                    // 防御性跳过，防把中断态误判为异常 Paused 空转。
                    si = si + 1;
                    continue;
                }
                if (run.Status == AITaskRunStatus.Pending) {
                    run.Start();
                }
                if (run.Status == AITaskRunStatus.Running) {
                    // A3 旁路注入：邮箱非空 → 拼入 prompt delta（增量 append，不动前缀稳定块，
                    // 保护 KV cache 复用；≤1 回合生效）。
                    List<AISubAgentMessage> msgs = run.DrainMessages();
                    if (msgs.Count > 0) {
                        run.NextPrompt = run.NextPrompt + this.BuildMessageDelta(msgs);
                    }
                    if (run.HeartbeatCount > 0 && run.IsStale(_heartbeatTimeoutMs)) {
                        run.Fail("DeadHeartbeat");
                    } else if (run.IsDurationExceeded()) {
                        run.Fail("TaskTimeout");
                    } else {
                        AIReply reply = await run.RunStepAsync(run.NextPrompt, linkedToken);
                        consumedSteps = consumedSteps + 1;
                        if (linkedToken.IsCancellationRequested) {
                            // A2 修取消被吞：撤单/ct 取消 → Cancelled（不被 reply.IsError 吞成 Failed）。
                            run.Cancel();
                        } else if (reply != null && reply.IsError) {
                            run.Fail(reply.ErrorKind != null && reply.ErrorKind != "" ? reply.ErrorKind : "SubAgentError");
                        } else if (reply != null && reply.Text != null && reply.Text != "") {
                            run.NextPrompt = reply.Text;
                        }
                        // 有界步数收口：满 MaxSteps 仍未完结 → Completed（与 AITaskRun.RunAsync 语义一致）。
                        if (run.Status == AITaskRunStatus.Running && run.Steps >= run.MaxSteps) {
                            run.Complete();
                        }
                    }
                } else if (run.Status == AITaskRunStatus.Paused) {
                    // 并行协调器不产生 Checkpoint 暂停；出现 Paused（异常路径）→ 收束，防循环空转。
                    run.Fail("SubAgentPaused");
                }
                si = si + 1;
            }
            // ④ reconcile：终结（Failed/Completed/Cancelled）同 tick 入账 + 即时释放租约。
            this.Reconcile(graph, inFlight);
            // A4 ⑤ 预算强制收束梯（TotalBudget 从「只算」到「强制」）：Σ子代理回合用量 > 预算 →
            // ① 停止新派发（本 tick 顶部已 gate）② wrap-up 旁路注入（宽限 1 回合）③ 宽限后强制
            // 中断 → 收束小结 ④ 未收束 → Failed(BudgetExceeded) + 小结 + 升级人（禁无界追加）。
            budgetExhausted = budget > 0 && consumedSteps >= budget;
            if (budgetExhausted) {
                _totalBudgetExceeded = true;
                if (inFlight.Count == 0) {
                    break;
                }
                if (!_wrapupInjected) {
                    this.InjectWrapUp(inFlight);
                    _wrapupInjected = true;
                } else {
                    this.ForceSettle(graph, inFlight, "BudgetExceeded");
                    break;
                }
            }
        }
        this.SettleRunState();
        return runs;
    }

    /// <summary>
    /// A2 撤单收束（subagent-management）：取消未启动 + 中断在飞 + 即时释放该 Rfc 全部租约。
    /// ① 中断在飞子代理（联动 <see cref="AISubAgentRun.Interrupt"/> → AITaskRun.Cancelled，
    /// 并记 subagent:interrupt 决策事件）；② 取消未启动工作项（Open/Blocked → Cancelled，
    /// 经 <see cref="AIRfcTaskGraph.MarkCancelled"/>，不进下一波）；③ 即时释放本波会话全部
    /// ToolPath 租约（被占路径可被后续批次/会话取得）；④ 取消的容器回收（<see cref="AISubAgentRun.Reap"/>
    /// → Dead）。处置选项（keep-wip / rollback）由调用方按场景协议执行，本方法只做收束。
    /// </summary>
    public async Task CancelPendingAsync(string rfcId) {
        _cancelRequested = true;
        if (_cancelCts != null) {
            _cancelCts.Cancel();
        }
        AIRfcTaskGraph? graph = _activeGraph;
        if (graph != null && !this.GraphMatchesRfc(graph, rfcId)) {
            return;
        }
        List<AISubAgentRun>? inFlight = _inflightRuns;
        if (inFlight != null) {
            int i = 0;
            while (i < inFlight.Count) {
                AISubAgentRun run = inFlight[i];
                if (run != null) {
                    run.Interrupt();
                    if (run.Session != null) {
                        run.Session.AppendDecisionEvent(
                            AIDecisionEventKind.SubagentInterrupt,
                            "workitem " + run.WorkItemId + " interrupted by cancel pending");
                    }
                }
                i = i + 1;
            }
        }
        if (graph != null) {
            List<AIRfcWorkItem> items = graph.Items;
            int j = 0;
            while (j < items.Count) {
                AIRfcWorkItem item = items[j];
                if (item.Status == AIRfcWorkItemStatus.Done
                    || item.Status == AIRfcWorkItemStatus.Failed
                    || item.Status == AIRfcWorkItemStatus.Cancelled) {
                    j = j + 1;
                    continue;
                }
                graph.MarkCancelled(item.WorkItemId, this.BuildCancelSummary(item.WorkItemId));
                j = j + 1;
            }
        }
        if (inFlight != null) {
            int k = 0;
            while (k < inFlight.Count) {
                AISubAgentRun run = inFlight[k];
                if (run != null) {
                    if (run.Session != null) {
                        _coordinator.ReleaseSession(run.Session.SessionId);
                    }
                    run.Reap();
                }
                k = k + 1;
            }
        }
    }

    /// <summary>
    /// A3 决策广播（subagent-management §4）：把决策挂入待决队列，reconcile 循环在下一
    /// tick 同 tick 消费——revision-changed → 全部在飞检查点 + 重建 ContextBlock 到新
    /// Revision + 租约重验（Scope 变冲突 → Failed + 必答小结；不变 → 继续）；wrap-up →
    /// 旁路注入「收束部分成果」（soft，不打断回合）。单线程宿主：本方法体同步完成
    /// （只存状态），实际干预在 reconcile 循环内，非多线程。
    /// </summary>
    public async Task PendingSyncDecisionAsync(AISubAgentDecision decision) {
        if (decision == null) {
            return;
        }
        // 广播决策携带新 Revision → 后续新派发的工作项以其为基准（重对齐前置）。
        if (decision.RfcRevision > 0) {
            _rfcRevision = decision.RfcRevision;
        }
        _pendingDecision = decision;
        _decisionPending = true;
    }

    /// <summary>
    /// A3 决策定向（subagent-management §4）：只作用于指定 run（RunId 或 WorkItemId 定位）。
    /// work-item-rescope → 定向重取 Scope 租约（后到拒绝 → Failed + 小结）；wrap-up →
    /// 旁路注入收束。单线程宿主：只投递待决，reconcile 同 tick 消费。
    /// </summary>
    public async Task SyncDecisionAsync(string runId, AISubAgentDecision decision) {
        AISubAgentRun? run = this.FindRun(runId);
        if (run == null || decision == null) {
            return;
        }
        run.PendingDecision = decision;
    }

    /// <summary>
    /// A3 旁路注入（subagent-management §4 soft 通道）：非打断消息挂到 run 邮箱
    /// （<see cref="AISubAgentRun.EnqueueMessage"/>），reconcile 在下次 RunStepAsync 前拼入
    /// prompt delta（增量 append，不动前缀稳定块，保护 KV cache 复用）。≤1 回合生效。
    /// </summary>
    public async Task EnqueueMessageAsync(string runId, AISubAgentMessage message) {
        AISubAgentRun? run = this.FindRun(runId);
        if (run == null || message == null) {
            return;
        }
        run.EnqueueMessage(message);
    }

    /// <summary>
    /// 按 runId（<see cref="AITaskRun.RunId"/> 或工作项 <see cref="AISubAgentRun.WorkItemId"/>）
    /// 定位在飞子代理容器；未找到 → null。
    /// </summary>
    public AISubAgentRun? FindRun(string runId) {
        List<AISubAgentRun>? inFlight = _inflightRuns;
        if (inFlight == null || runId == null || runId == "") {
            return null;
        }
        int i = 0;
        while (i < inFlight.Count) {
            AISubAgentRun run = inFlight[i];
            if (run != null && (run.RunId == runId || run.WorkItemId == runId)) {
                return run;
            }
            i = i + 1;
        }
        return null;
    }

    /// <summary>
    /// reconcile：终结的子代理同 tick 入账（必答小结 + MarkDone/MarkCancelled）并释放其会话全部
    /// 租约（<see cref="AICoordinator.ReleaseSession"/>——不再延迟到 RunAllAsync 尾部）。取消
    /// （Cancelled）的容器一并回收（Dead）。存活者收拢。
    /// </summary>
    private void Reconcile(AIRfcTaskGraph graph, List<AISubAgentRun> inFlight) {
        List<AISubAgentRun> alive = new List<AISubAgentRun>();
        int i = 0;
        while (i < inFlight.Count) {
            AISubAgentRun run = inFlight[i];
            if (run.Status == AITaskRunStatus.Failed
                || run.Status == AITaskRunStatus.Completed
                || run.Status == AITaskRunStatus.Cancelled) {
                this.FinalizeRun(graph, run);
                if (run.Session != null) {
                    _coordinator.ReleaseSession(run.Session.SessionId);
                }
                if (run.Status == AITaskRunStatus.Cancelled) {
                    run.Reap();
                }
            } else {
                alive.Add(run);
            }
            i = i + 1;
        }
        inFlight.Clear();
        int j = 0;
        while (j < alive.Count) {
            inFlight.Add(alive[j]);
            j = j + 1;
        }
    }

    /// <summary>
    /// 终结入账：必答小结（无小结则构建）。按终态分派——Cancelled →
    /// <see cref="AIRfcTaskGraph.MarkCancelled"/>（未完结，汇总门红）；Failed →
    /// <see cref="AIRfcTaskGraph.MarkFailed"/>（失败信号持久承载，跨会话可查，不折叠成 Done）；
    /// 其余（Completed）→ <see cref="AIRfcTaskGraph.MarkDone"/>。A5：终结时聚合 token/轮次入
    /// <see cref="TotalUsage"/>（subagent:usage 决策事件单轨入账）。
    /// </summary>
    private void FinalizeRun(AIRfcTaskGraph graph, AISubAgentRun run) {
        if (run == null) {
            return;
        }
        this.AccumulateUsage(run);
        AIWorkSummary summary = this.BuildSummary(run);
        if (!run.HasSummary) {
            run.SetSummary(summary);
        }
        AIWorkSummary effective = run.Summary != null ? run.Summary : summary;
        if (run.Status == AITaskRunStatus.Cancelled) {
            graph.MarkCancelled(run.WorkItemId, effective);
        } else if (run.Status == AITaskRunStatus.Failed) {
            graph.MarkFailed(run.WorkItemId, effective);
        } else {
            graph.MarkDone(run.WorkItemId, effective);
        }
    }

    /// <summary>
    /// 构建子代理必答小结。写面冲突（惰性租约门 IsBlocked）→ 后到拒绝小结（含 ToolPath 明细）；
    /// 其余按终结状态折叠（Completed = 正常；Failed/Cancelled = 绕过 + 原因，仍必答）。
    /// </summary>
    private AIWorkSummary BuildSummary(AISubAgentRun run) {
        if (run == null) {
            return new AIWorkSummary("", "无", "无", "无");
        }
        if (run.LeaseGate != null && run.LeaseGate.IsBlocked) {
            AIWorkSummary conflict = new AIWorkSummary(
                run.WorkItemId,
                "ToolPath 写面冲突：写面被拒（" + run.LeaseGate.BlockedPath + "，持有者 " + run.LeaseGate.BlockedHolder + "）",
                "后到拒绝：共享写面被其它子代理占用",
                "无（写被拒，未落盘）");
            conflict.Difficulty = "绕过";
            conflict.Findings = "ToolPath 后到拒绝（写面 Acquired=false，首次写或决策重验），工作项 Failed，禁第二把锁";
            return conflict;
        }
        string statusName = AIParallelCoordinator.StatusName(run.Status);
        string verify = "status=" + statusName;
        string diff = "无";
        string find = "无";
        if (run.Status != AITaskRunStatus.Completed) {
            diff = "绕过";
            string progress = run.Progress != null && run.Progress != "" ? run.Progress : statusName;
            find = "子代理会话未按预期完结：" + progress;
        }
        AIWorkSummary summary = new AIWorkSummary(
            run.WorkItemId,
            "子代理会话执行 " + run.Steps + " 步",
            "设计 ✓ · 需求 ✓",
            verify);
        summary.Difficulty = diff;
        summary.Findings = find;
        return summary;
    }

    /// <summary>撤单小结（未启动/未完结工作项入账用；必答五字段）。</summary>
    private AIWorkSummary BuildCancelSummary(string workItemId) {
        AIWorkSummary summary = new AIWorkSummary(
            workItemId,
            "撤单收束：工作项取消，未启动/未完结",
            "n/a（撤单）",
            "status=Cancelled");
        summary.Difficulty = "绕过";
        summary.Findings = "撤单收束（CancelPendingAsync）：Cancelled 计入未完结，汇总门红（Pending≠Passed）";
        return summary;
    }

    /// <summary>当前活动图是否属于 <paramref name="rfcId"/>（撤单守卫；非该 Rfc 不误伤）。</summary>
    private bool GraphMatchesRfc(AIRfcTaskGraph graph, string rfcId) {
        if (rfcId == null || rfcId == "") {
            return true;
        }
        List<AIRfcWorkItem> items = graph.Items;
        if (items != null && items.Count > 0 && items[0] != null) {
            return items[0].RfcId == rfcId;
        }
        return true;
    }

    /// <summary>RunAllAsync 收尾/早退统一清理 A2 运行态（防跨批次残留）。</summary>
    private void SettleRunState() {
        if (_cancelCts != null) {
            _cancelCts.Dispose();
            _cancelCts = null;
        }
        _activeGraph = null;
        _inflightRuns = null;
        _cancelRequested = false;
        _pendingDecision = null;
        _decisionPending = false;
    }

    /// <summary>
    /// A4 预算强制收束终步（③④）：把在飞全部标记 Failed（<paramref name="reason"/>）+ 必答小结 +
    /// 释放租约，清空在飞；剩余未启动工作项保持未启动（升级人）。禁无界追加。
    /// </summary>
    private void ForceSettle(AIRfcTaskGraph graph, List<AISubAgentRun> inFlight, string reason) {
        int i = 0;
        while (i < inFlight.Count) {
            AISubAgentRun run = inFlight[i];
            if (run.Status == AITaskRunStatus.Running || run.Status == AITaskRunStatus.Pending) {
                run.Fail(reason);
            }
            this.FinalizeRun(graph, run);
            if (run.Session != null) {
                _coordinator.ReleaseSession(run.Session.SessionId);
            }
            i = i + 1;
        }
        inFlight.Clear();
    }

    /// <summary>
    /// A4 预算收束 ②：对在飞 Running/Pending 子代理旁路注入 wrap-up（soft，下回合拼入 prompt
    /// delta，不打断当前回合；宽限 1 回合收束部分成果）。写 subagent:usage 决策事件入轨迹。
    /// </summary>
    private void InjectWrapUp(List<AISubAgentRun> inFlight) {
        int i = 0;
        while (i < inFlight.Count) {
            AISubAgentRun run = inFlight[i];
            if (run != null && (run.Status == AITaskRunStatus.Running || run.Status == AITaskRunStatus.Pending)) {
                AISubAgentMessage msg = new AISubAgentMessage();
                msg.Kind = "wrap-up";
                msg.Interruptive = false;
                msg.Payload = "预算已耗尽，请收束部分成果";
                run.EnqueueMessage(msg);
                if (run.Session != null) {
                    run.Session.AppendDecisionEvent(
                        AIDecisionEventKind.SubagentUsage,
                        "budget exhausted, wrap-up injected for " + run.WorkItemId,
                        "TotalBudget");
                }
            }
            i = i + 1;
        }
    }

    /// <summary>
    /// A5 成本核算：聚合单 run 的 token 用量与轮次入 <see cref="TotalUsage"/> /
    /// <see cref="TotalTurns"/>，并写 subagent:usage 决策事件（单轨，复盘底座）。终结时调用一次。
    /// </summary>
    private void AccumulateUsage(AISubAgentRun run) {
        if (run == null) {
            return;
        }
        _totalTurns = _totalTurns + run.Steps;
        if (run.Session == null) {
            return;
        }
        AITokenUsage u = run.Session.TotalUsage;
        if (u == null) {
            return;
        }
        _totalUsage.PromptTokens = _totalUsage.PromptTokens + u.PromptTokens;
        _totalUsage.CompletionTokens = _totalUsage.CompletionTokens + u.CompletionTokens;
        _totalUsage.TotalTokens = _totalUsage.TotalTokens + u.TotalTokens;
        _totalUsage.CacheReadTokens = _totalUsage.CacheReadTokens + u.CacheReadTokens;
        _totalUsage.CacheCreationTokens = _totalUsage.CacheCreationTokens + u.CacheCreationTokens;
        run.Session.AppendDecisionEvent(
            AIDecisionEventKind.SubagentUsage,
            "workitem " + run.WorkItemId + " tokens=" + u.TotalTokens + " turns=" + run.Steps,
            "subagent usage report");
    }

    /// <summary>A5 成本核算：重置聚合用量（RunAllAsync 起始；单批口径）。</summary>
    private void ResetUsage() {
        _totalUsage.PromptTokens = 0;
        _totalUsage.CompletionTokens = 0;
        _totalUsage.TotalTokens = 0;
        _totalUsage.CacheReadTokens = 0;
        _totalUsage.CacheCreationTokens = 0;
        _totalTurns = 0;
        _totalBudgetExceeded = false;
    }

    /// <summary>
    /// 组装任务专属上下文块：全局前缀稳定（共用需求/设计/验收摘要，跨子代理复用 KV cache）
    /// + 本工作项的 Scope / DependsOn 摘要 / 目标清单。
    /// </summary>
    private string BuildTaskContext(AIRfcWorkItem workItem, string contextBlock) {
        return this.BuildTaskContext(workItem, contextBlock, _rfcRevision);
    }

    /// <summary>
    /// 组装任务专属上下文块（显式 Revision 版：决策重对齐用新 Revision 重建，非启动时快照）。
    /// </summary>
    private string BuildTaskContext(AIRfcWorkItem workItem, string contextBlock, int revision) {
        string prefix = _prefixContext != null ? _prefixContext : "";
        string block = "[airfc v" + revision + "]\n" + prefix;
        block = block + "\n[workitem " + workItem.WorkItemId + "]\n";
        block = block + "scope: " + AIParallelCoordinator.Join(workItem.Scope) + "\n";
        block = block + "dependsOn: " + AIParallelCoordinator.Join(workItem.DependsOn) + "\n";
        if (contextBlock != null && contextBlock != "") {
            block = block + "task: " + contextBlock + "\n";
        }
        return block;
    }

    /// <summary>
    /// A3 待决决策处理：先广播（<see cref="PendingSyncDecisionAsync"/> 投递）后定向
    /// （<see cref="SyncDecisionAsync"/> 投递到 run.PendingDecision）。单线程宿主内同步消费，
    /// 不另起线程/锁。
    /// </summary>
    private void ProcessPendingDecisions(AIRfcTaskGraph graph, List<AISubAgentRun> inFlight) {
        AISubAgentDecision? broadcast = _pendingDecision;
        if (_decisionPending && broadcast != null) {
            int i = 0;
            while (i < inFlight.Count) {
                AISubAgentRun run = inFlight[i];
                if (this.DecisionTargets(run, broadcast)) {
                    this.ApplyDecision(graph, run, broadcast);
                }
                i = i + 1;
            }
            _pendingDecision = null;
            _decisionPending = false;
        }
        int j = 0;
        while (j < inFlight.Count) {
            AISubAgentRun run = inFlight[j];
            AISubAgentDecision? targeted = run.PendingDecision;
            if (targeted != null) {
                run.PendingDecision = null;
                this.ApplyDecision(graph, run, targeted);
            }
            j = j + 1;
        }
    }

    /// <summary>决策是否作用于该 run（TargetWorkItems 空 = 广播全部；否则按工作项 Id 定向）。</summary>
    private bool DecisionTargets(AISubAgentRun run, AISubAgentDecision decision) {
        if (run == null || decision == null) {
            return false;
        }
        List<string> targets = decision.TargetWorkItems;
        if (targets == null || targets.Count == 0) {
            return true;
        }
        int i = 0;
        while (i < targets.Count) {
            if (targets[i] == run.WorkItemId) {
                return true;
            }
            i = i + 1;
        }
        return false;
    }

    /// <summary>
    /// 单条决策应用（revision-changed / work-item-rescope / wrap-up；"cancel" 走
    /// <see cref="CancelPendingAsync"/>，A2 撤单路径）。已终态 run 不回退。
    /// </summary>
    private void ApplyDecision(AIRfcTaskGraph graph, AISubAgentRun run, AISubAgentDecision decision) {
        if (run == null || decision == null) {
            return;
        }
        if (run.Status == AITaskRunStatus.Completed
            || run.Status == AITaskRunStatus.Failed
            || run.Status == AITaskRunStatus.Cancelled) {
            return;
        }
        string kind = decision.Kind != null ? decision.Kind : "";
        if (kind == "revision-changed") {
            this.ApplyRevisionChanged(graph, run, decision);
        } else if (kind == "work-item-rescope") {
            this.ApplyWorkItemRescope(graph, run, decision);
        } else if (kind == "wrap-up") {
            // 预算压力旁路注入（soft）：不打断回合，下回合拼入 prompt delta 收束部分成果。
            AISubAgentMessage msg = new AISubAgentMessage();
            msg.Kind = "wrap-up";
            msg.Interruptive = false;
            msg.RfcRevision = decision.RfcRevision;
            msg.Payload = decision.Reason != null && decision.Reason != ""
                ? decision.Reason : "预算压力：请收束部分成果";
            run.EnqueueMessage(msg);
        }
    }

    /// <summary>
    /// A3 revision-changed 重对齐（subagent-management §4）：检查点（Running → Interrupted）→
    /// 重建 ContextBlock 到新 Revision → 租约重验（Scope 变冲突 → Failed + 必答小结；不变 →
    /// 继续）。重对齐后以新 Revision 为写回基准。subagent:sync 决策事件入该 run 会话轨迹。
    /// </summary>
    private void ApplyRevisionChanged(AIRfcTaskGraph graph, AISubAgentRun run, AISubAgentDecision decision) {
        AIRfcWorkItem? item = AIRfcWorkItem.FindItem(graph.Items, run.WorkItemId);
        if (item == null) {
            return;
        }
        run.CheckpointInterrupt();
        string title = item.Title != null ? item.Title : run.WorkItemId;
        int newRevision = decision.RfcRevision > 0 ? decision.RfcRevision : (_rfcRevision > 0 ? _rfcRevision : run.RfcRevision);
        string newContext = this.BuildTaskContext(item, title, newRevision);
        run.Realign(newContext, newRevision);
        bool leaseOk = run.RevalidateLease(item.Scope);
        if (run.Session != null) {
            string detail = "workitem " + run.WorkItemId + " realigned v" + newRevision
                + (leaseOk ? " lease ok" : " lease conflict " + this.BlockedPathForAudit(run));
            run.Session.AppendDecisionEvent(AIDecisionEventKind.SubagentSync, detail, decision.Reason);
        }
        if (!leaseOk) {
            // 租约重验冲突：Scope 变且新增写面被其它会话持有 → 后到拒绝 → Failed + 必答小结。
            run.ClearInterrupt();
            run.Fail("LeaseConflict");
        } else {
            run.ResumeAfterSync();
            // 重对齐 delta：增量拼入下回合提示（不动前缀稳定块；KV cache 前缀复用）。
            run.NextPrompt = "[subagent sync: revision-changed → v" + newRevision + "]\n"
                + "AIRfc 已升版，请按更新后的上下文继续工作项 " + run.WorkItemId + "：\n"
                + newContext;
        }
    }

    /// <summary>
    /// A3 work-item-rescope 定向重对齐（subagent-management §4）：检查点 → 重取 Scope 租约
    /// （新增写面路径被其它会话持有 → 后到拒绝 → Failed + 必答小结）。subagent:sync 事件入轨迹。
    /// </summary>
    private void ApplyWorkItemRescope(AIRfcTaskGraph graph, AISubAgentRun run, AISubAgentDecision decision) {
        AIRfcWorkItem? item = AIRfcWorkItem.FindItem(graph.Items, run.WorkItemId);
        if (item == null) {
            return;
        }
        run.CheckpointInterrupt();
        string title = item.Title != null ? item.Title : run.WorkItemId;
        int revision = decision.RfcRevision > 0 ? decision.RfcRevision : run.RfcRevision;
        string newContext = this.BuildTaskContext(item, title, revision);
        run.Realign(newContext, revision);
        bool leaseOk = run.RevalidateLease(item.Scope);
        if (run.Session != null) {
            string detail = "workitem " + run.WorkItemId + " rescope "
                + (leaseOk ? "lease reacquired" : "lease conflict " + this.BlockedPathForAudit(run));
            run.Session.AppendDecisionEvent(AIDecisionEventKind.SubagentSync, detail, decision.Reason);
        }
        if (!leaseOk) {
            run.ClearInterrupt();
            run.Fail("LeaseConflict");
        } else {
            run.ResumeAfterSync();
            run.NextPrompt = "[subagent sync: work-item-rescope]\n"
                + "工作项 " + run.WorkItemId + " Scope 已更新，请按新写面继续：\n"
                + newContext;
        }
    }

    /// <summary>租约重验冲突路径审计（未冲突 → 空串）。</summary>
    private string BlockedPathForAudit(AISubAgentRun run) {
        if (run.LeaseGate == null || !run.LeaseGate.IsBlocked) {
            return "";
        }
        return "path=" + run.LeaseGate.BlockedPath + " holder=" + run.LeaseGate.BlockedHolder;
    }

    /// <summary>
    /// 旁路注入 prompt delta 折叠（soft 通道）：增量 append 到当前 NextPrompt 之后，
    /// 不动前缀稳定块；wrap-up 专用前缀便于子代理识别收束指令。
    /// </summary>
    private string BuildMessageDelta(List<AISubAgentMessage> messages) {
        string delta = "";
        if (messages == null || messages.Count == 0) {
            return delta;
        }
        int i = 0;
        while (i < messages.Count) {
            AISubAgentMessage m = messages[i];
            if (m == null) {
                i = i + 1;
                continue;
            }
            string kind = m.Kind != null && m.Kind != "" ? m.Kind : "message";
            string payload = m.Payload != null ? m.Payload : "";
            delta = delta + "\n[subagent " + kind + "]\n" + payload + "\n";
            i = i + 1;
        }
        return delta;
    }

    private bool HasRfcLease() {
        return _rfcLeaseRfcId != null && _rfcLeaseRfcId != "";
    }

    private static string StatusName(AITaskRunStatus s) {
        if (s == AITaskRunStatus.Running) { return "Running"; }
        if (s == AITaskRunStatus.Paused) { return "Paused"; }
        if (s == AITaskRunStatus.Completed) { return "Completed"; }
        if (s == AITaskRunStatus.Failed) { return "Failed"; }
        if (s == AITaskRunStatus.Cancelled) { return "Cancelled"; }
        return "Pending";
    }

    private static string Join(List<string> items) {
        if (items == null || items.Count == 0) {
            return "-";
        }
        string result = "";
        int i = 0;
        while (i < items.Count) {
            if (i > 0) {
                result = result + ", ";
            }
            result = result + items[i];
            i = i + 1;
        }
        return result;
    }
}
