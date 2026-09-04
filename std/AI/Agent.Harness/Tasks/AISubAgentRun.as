// RFC 043 P3（parallel-subagents §3.4 / subagent-management A2 + A3）：子代理运行容器 —— 可继承
// Arc.Agent.AITaskRun。每个工作项一个独立 AISession；WorkItemId + RfcRevision（启动时只读
// 快照）+ 必答小结。A2 增精化生命周期状态 <see cref="AISubAgentState"/>（Spawn→Running→
// (Interrupted|Paused|Completed|Failed|Cancelled)，回收 → Dead），与 AITaskRunStatus 映射。
// A3 增：旁路注入邮箱 <see cref="PendingMessages"/>（soft 消息下回合拼入 prompt delta）；
// 决策重对齐 <see cref="CheckpointInterrupt"/> / <see cref="ResumeAfterSync"/> /
// <see cref="Realign"/> / <see cref="RevalidateLease"/>（检查点 → 新 Revision 上下文 →
// 租约重验 → 继续或 Failed/Cancelled）。
namespace Arc.Agent.Harness;
using Arc;
using Arc.Agent;
using Arc.Collections;

/// <summary>
/// 并行子代理运行容器：扩展 <see cref="AITaskRun"/>，承载单个工作项的执行。
/// 启动时持有 AIRfc 只读快照的 <see cref="RfcRevision"/>（写回冲突以它为基准；重对齐后
/// 刷新为新 Revision）；完成后必答 <see cref="Summary"/>（<see cref="SetSummary"/> 前置
/// null 校验——无小结不得 MarkDone）。精化状态 <see cref="State"/> 由 <see cref="Spawn"/> /
/// <see cref="Interrupt"/> / <see cref="CheckpointInterrupt"/> / <see cref="Reap"/> 与基类
/// 状态迁移共同驱动。
/// </summary>
public class AISubAgentRun : AITaskRun {
    private string _workItemId;
    private int _rfcRevision;
    private AIWorkSummary? _summary;
    private string _contextBlock;
    // 精化状态标记：Spawned（已派发未 Start）/ Interrupted（决策中断或撤单收束）/ Dead（已回收）。
    private bool _spawned;
    private bool _interrupted;
    private bool _dead;
    // A1 惰性租约门（首次真实写前取 ToolPath 租约；null = 未装配）与下一回合提示词。
    private AISubAgentLeaseGate? _leaseGate;
    private string _nextPrompt;
    // A3 旁路注入邮箱（soft 消息；reconcile 下回合拼入 prompt delta）与定向待决决策。
    private List<AISubAgentMessage> _pendingMessages;
    private AISubAgentDecision? _pendingDecision;
    // A3 决策检查点快照（CheckpointInterrupt 捕获；ResumeAfterSync 恢复续跑）。
    private AITaskRunSnapshot? _checkpointSnapshot;

    public AISubAgentRun(AISession session, int maxSteps) : base(session, maxSteps) {
        _workItemId = "";
        _rfcRevision = 0;
        _summary = null;
        _contextBlock = "";
        _spawned = false;
        _interrupted = false;
        _dead = false;
        _leaseGate = null;
        _nextPrompt = "";
        _pendingMessages = new List<AISubAgentMessage>();
        _pendingDecision = null;
        _checkpointSnapshot = null;
    }

    /// <summary>所服务工作项 Id。</summary>
    public string WorkItemId {
        get { return _workItemId; }
        set { _workItemId = value != null ? value : ""; }
    }

    /// <summary>启动时 AIRfc 只读快照的 Revision（写回冲突以它为基准）。</summary>
    public int RfcRevision {
        get { return _rfcRevision; }
        set { _rfcRevision = value; }
    }

    /// <summary>必答小结（五字段）；未设置（null）时 <see cref="HasSummary"/> 为 false。</summary>
    public AIWorkSummary? Summary {
        get { return _summary; }
    }

    /// <summary>是否已设置必答小结。</summary>
    public bool HasSummary {
        get { return _summary != null; }
    }

    /// <summary>任务专属上下文块（全局前缀稳定 + 本工作项 Scope/DependsOn/目标清单）。</summary>
    public string ContextBlock {
        get { return _contextBlock; }
        set { _contextBlock = value != null ? value : ""; }
    }

    /// <summary>
    /// 精化生命周期状态（A2）。由 Spawn/Interrupt/Reap 标记与基类 <see cref="AITaskRun.Status"/>
    /// 映射：未 Spawn → Pending；已 Spawn 未 Start → Spawned；被中断 → Interrupted；已回收 → Dead；
    /// 其余与 AITaskRunStatus 一一对应（Running/Paused/Completed/Failed/Cancelled）。
    /// </summary>
    public AISubAgentState State {
        get {
            if (_dead) {
                return AISubAgentState.Dead;
            }
            if (_interrupted) {
                return AISubAgentState.Interrupted;
            }
            if (!_spawned) {
                return AISubAgentState.Pending;
            }
            AITaskRunStatus s = this.Status;
            if (s == AITaskRunStatus.Pending) { return AISubAgentState.Spawned; }
            if (s == AITaskRunStatus.Running) { return AISubAgentState.Running; }
            if (s == AITaskRunStatus.Paused) { return AISubAgentState.Paused; }
            if (s == AITaskRunStatus.Completed) { return AISubAgentState.Completed; }
            if (s == AITaskRunStatus.Failed) { return AISubAgentState.Failed; }
            return AISubAgentState.Cancelled;
        }
    }

    /// <summary>派发即标记：Pending → Spawned（已持租约，尚未 Start）。</summary>
    public void Spawn() {
        if (_spawned) {
            return;
        }
        _spawned = true;
    }

    /// <summary>
    /// 撤单收束中断：Spawned/Running/Paused/Pending → Interrupted 并联动基类
    /// <see cref="AITaskRun.Cancel"/>（终态 Completed/Failed/Cancelled/Dead 不回退）——
    /// 未启动即中断的工作项也须落终态 Cancelled（否则 Reconcile 不判终结、不 MarkCancelled）。
    /// 在飞会话的停止由协调器经联动 CTS 驱动。
    /// </summary>
    public void Interrupt() {
        AITaskRunStatus s = this.Status;
        if (s == AITaskRunStatus.Completed || s == AITaskRunStatus.Failed || s == AITaskRunStatus.Cancelled) {
            return;
        }
        if (_dead) {
            return;
        }
        _interrupted = true;
        this.Cancel();
    }

    /// <summary>回收：会话租约已释放，容器不可再用（State → Dead）。</summary>
    public void Reap() {
        _dead = true;
    }

    /// <summary>惰性 ToolPath 租约门（A1；null = 未装配）。</summary>
    public AISubAgentLeaseGate? LeaseGate {
        get { return _leaseGate; }
    }

    /// <summary>装配惰性租约门（A1：派发时以工作项 Scope 为声明写面；sandbox 首次真实写前经门取租约）。</summary>
    public void SetLeaseGate(AISubAgentLeaseGate gate) {
        _leaseGate = gate;
    }

    /// <summary>下一回合提示词（reconcile 逐回合推进；首轮 = ContextBlock，后续 = 上一回复）。</summary>
    public string NextPrompt {
        get { return _nextPrompt; }
        set { _nextPrompt = value != null ? value : ""; }
    }

    /// <summary>
    /// 旁路注入邮箱（A3 soft 通道）：非打断消息挂此队列，reconcile 在下次
    /// <see cref="AITaskRun.RunStepAsync"/> 前经 <see cref="DrainMessages"/> 拼入 prompt
    /// delta（增量 append，不动前缀稳定块）。定向待决决策由管理器经 <see cref="PendingDecision"/> 投递。
    /// </summary>
    public List<AISubAgentMessage> PendingMessages {
        get { return _pendingMessages; }
    }

    /// <summary>定向待决决策（A3：SyncDecisionAsync 投递；reconcile 同 tick 消费）。</summary>
    public AISubAgentDecision? PendingDecision {
        get { return _pendingDecision; }
        set { _pendingDecision = value; }
    }

    /// <summary>
    /// 旁路注入（A3 soft 通道）：挂入邮箱，不打断当前回合（下回合边界生效）。
    /// null 消息忽略。
    /// </summary>
    public void EnqueueMessage(AISubAgentMessage message) {
        if (message == null) {
            return;
        }
        _pendingMessages.Add(message);
    }

    /// <summary>取走并清空邮箱（reconcile 回合前消费；返回拷贝，不破坏外部枚举）。</summary>
    public List<AISubAgentMessage> DrainMessages() {
        List<AISubAgentMessage> drained = new List<AISubAgentMessage>();
        int i = 0;
        while (i < _pendingMessages.Count) {
            drained.Add(_pendingMessages[i]);
            i = i + 1;
        }
        _pendingMessages.Clear();
        return drained;
    }

    /// <summary>
    /// A3 决策同步检查点打断：Running → 基类 <see cref="AITaskRun.Checkpoint"/>（Paused +
    /// 快照捕获）并置 Interrupted 标记（State → Interrupted）。Spawned（未启动）仅重建
    /// 上下文（无需检查点）；已终态 / 已回收不回退；异常 Paused 不置标记（调用方按异常
    /// 路径收束，防恢复失败死循环）。非终态语义（与 A2 撤单 <see cref="Interrupt"/>
    /// 的终态 Cancelled 区分）：重对齐 + 租约重验通过后经 <see cref="ResumeAfterSync"/> 恢复。
    /// </summary>
    public void CheckpointInterrupt() {
        AITaskRunStatus s = this.Status;
        if (s == AITaskRunStatus.Completed || s == AITaskRunStatus.Failed || s == AITaskRunStatus.Cancelled) {
            return;
        }
        if (_dead) {
            return;
        }
        if (s == AITaskRunStatus.Running) {
            _checkpointSnapshot = this.Checkpoint();
            _interrupted = true;
        }
    }

    /// <summary>清除决策中断标记（重验冲突收束后状态回正，State 以基类终态为准）。</summary>
    public void ClearInterrupt() {
        _interrupted = false;
    }

    /// <summary>
    /// A3 决策重对齐后恢复：清 Interrupted 标记；若为检查点 Paused → 基类
    /// <see cref="AITaskRun.Resume"/>（载入快照 → Running，会话上下文恢复续跑）。
    /// </summary>
    public void ResumeAfterSync() {
        _interrupted = false;
        if (this.Status == AITaskRunStatus.Paused && _checkpointSnapshot != null) {
            this.Resume(_checkpointSnapshot);
        }
        _checkpointSnapshot = null;
    }

    /// <summary>
    /// A3 决策重对齐：把任务专属上下文块重建到新 Revision 并刷新 <see cref="RfcRevision"/>
    /// （重对齐后写回冲突以新版本为基准）。
    /// </summary>
    public void Realign(string contextBlock, int revision) {
        _contextBlock = contextBlock != null ? contextBlock : "";
        if (revision > 0) {
            _rfcRevision = revision;
        }
    }

    /// <summary>
    /// A3 租约重验：把惰性租约门声明写面更新为新 Scope 并重取新增路径租约（幂等）；
    /// 新增路径被其它会话持有 → false（后到拒绝 → 宿主把工作项标记 Failed + 必答小结）。
    /// 未装配租约门 → 平凡通过。
    /// </summary>
    public bool RevalidateLease(List<string> scope) {
        if (_leaseGate == null) {
            return true;
        }
        return _leaseGate.Revalidate(scope);
    }

    /// <summary>
    /// 必答小结写入：null → ArgumentException（无小结不得 MarkDone / 不得宣称完成）。
    /// </summary>
    public void SetSummary(AIWorkSummary summary) {
        if (summary == null) {
            throw new ArgumentException("AISubAgentRun summary is required");
        }
        _summary = summary;
    }
}
