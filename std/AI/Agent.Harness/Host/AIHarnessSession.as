// RFC 043：Harness 会话薄壳 — 持有 AIRfc + DoD；决策轨迹经 Agent 会话事件面（M5–M6 单轨）。
// 终态不把本类当公共聚合根；终端工程只组装。
// S0 增补：AIRfc 持久化（SaveRfc/RestoreRfc，落盘 target/scratch/arcagent-state/，禁源码树）+ 
// AIRfc 生命周期新态会话包装（Contend/Resolve/Freeze/Unfreeze/Close/Cancel）。
namespace Arc.Agent.Harness;
using Arc;
using Arc.Agent;
using Arc.Collections;
using Arc.IO;
using Arc.Text.Json;

/// <summary>
/// Harness 基座组合状态：持有 AIRfc 事实源、DoD 编排器；决策轨迹经
/// <see cref="AISession.AppendDecisionEvent"/> 写入 Agent 会话事件面（禁独立事件日志双轨）。
/// 终端工程（examples/ArcAgent）只组装，不重复实现质量门。
/// </summary>
public class AIHarnessSession {
    private const string StateRelDir = "target/scratch/arcagent-state";
    private const string StateFile = "airfc.json";
    private const string PlanFile = "plan.json";

    private string _project;
    private AIRfcRuntime _runtime;
    private AIRfc? _rfc;
    private AISession? _session;
    private AIDoDOrchestrator _dod;
    private AIPlanGate? _planGate;
    private AICheckpointStore _checkpoints;
    private bool _acceptanceGateEnabled;
    // A3 决策广播钩子：ReviseRfc 成功后向在飞子代理广播 revision-changed（接线方 =
    // 持有 AIParallelCoordinator 的宿主；null = 无并行在飞，不广播）。
    private Action<AISubAgentDecision>? _rfcRevisionChanged;
    // B1：本会话的冲突仲裁持有者 id（AttachCoordinator 登记；空 = 未参与跨会话仲裁）。
    private string _holderId;

    public AIHarnessSession(string project, IAIDoDGateEvaluator evaluator) {
        _project = AIHarnessSession.ResolveRoot(project);
        _runtime = new AIRfcRuntime();
        _rfc = null;
        _session = null;
        _dod = new AIDoDOrchestrator(project, evaluator);
        _planGate = null;
        _checkpoints = new AICheckpointStore(project);
        _acceptanceGateEnabled = false;
        _holderId = "";
    }

    public AIRfcRuntime Runtime {
        get { return _runtime; }
    }

    /// <summary>
    /// 共享运行时（B1 多方会话场景）：替换内部 <see cref="AIRfcRuntime"/>，使多会话共享同一
    /// AIRfc 登记表 + <see cref="AIConflictResolver"/>（L2 冲突跨会话可见）。须在挂协调器前
    /// 调用（<see cref="AttachCoordinator"/> 会转发到新运行时）。
    /// </summary>
    public void AttachRuntime(AIRfcRuntime runtime) {
        if (runtime == null) {
            return;
        }
        _runtime = runtime;
    }

    /// <summary>
    /// L2 冲突仲裁器（方案 B B1）：机器检测记录 + 人 CCB 裁决唯一入口。经运行时共享——
    /// 绑定同一运行时/协调器的会话看到同一冲突表。
    /// </summary>
    public AIConflictResolver Conflicts {
        get { return _runtime.Conflicts; }
    }

    /// <summary>
    /// 挂载冲突织物协调器（RFC 043 M5）：RfcSpec 租约经 <see cref="AICoordinator"/> 接线，
    /// 转发至 <see cref="AIRfcRuntime"/>；绑定 AICoordinator + 持有者（会话 id）。
    /// 未挂载 = 不参与跨会话仲裁（进程内登记）。
    /// </summary>
    public void AttachCoordinator(AICoordinator coordinator, string holderId) {
        _holderId = holderId != null ? holderId : "";
        _runtime.AttachCoordinator(coordinator, _holderId);
    }

    /// <summary>
    /// 采用共享运行时中已有的 Active AIRfc 为当前需求本尊（B1 多方场景：后到方先 Select 再
    /// Revise）。无 Active 版 → false。
    /// </summary>
    public bool SelectRfc(string rfcId) {
        AIRfc? rfc = _runtime.GetActive(rfcId);
        if (rfc == null) {
            return false;
        }
        _rfc = rfc;
        return true;
    }

    /// <summary>当前 Active AIRfc（需求本尊）。</summary>
    public AIRfc? Rfc {
        get { return _rfc; }
    }

    /// <summary>当前绑定的决策轨迹目标会话；null = 未接线（轨迹事件不落盘）。</summary>
    public AISession? BoundSession {
        get { return _session; }
    }

    public AIDoDOrchestrator DoD {
        get { return _dod; }
    }

    /// <summary>
    /// A3 纠偏广播钩子：<see cref="ReviseRfc"/> 成功升版后显式触发（携带 revision-changed
    /// 决策，TargetWorkItems 空 = 广播全部在飞子代理）。由持有子代理管理器
    /// （<see cref="AIParallelCoordinator"/>）的宿主接线到 <c>PendingSyncDecisionAsync</c>；
    /// 未接线 = 无并行在飞，/revise 不广播（单会话语义不变）。
    /// </summary>
    public Action<AISubAgentDecision>? RfcRevisionChanged {
        get { return _rfcRevisionChanged; }
        set { _rfcRevisionChanged = value; }
    }

    /// <summary>绿点快照存储（RFC 043 绿点/回滚协议；工作区状态落盘 target/scratch/arc-checkpoints）。</summary>
    public AICheckpointStore Checkpoints {
        get { return _checkpoints; }
    }

    /// <summary>当前绑定的计划门闩；null = 未挂载（DoD 完成不回写计划状态）。</summary>
    public AIPlanGate? PlanGate {
        get { return _planGate; }
    }

    /// <summary>接线计划门闩（RFC 038 §12 M8）：DoD D0–D7 全勾后经它把计划转入 Completed。</summary>
    public void AttachPlanGate(AIPlanGate gate) {
        _planGate = gate;
    }

    /// <summary>
    /// 启用 Acceptance 先行门闩（RFC 043 场景 1.1）：启用后，未定义可验收断言
    /// （Scenarios / Assertions 均空）的 AIRfc 拒绝 <see cref="AttachPlan"/>——模糊需求
    /// 须先经澄清向导补齐验收，禁止「先实现后补验收」。REPL 方向环装配时启用。
    /// </summary>
    public void EnableAcceptanceGate() {
        _acceptanceGateEnabled = true;
    }

    /// <summary>Acceptance 先行门闩是否启用。</summary>
    public bool AcceptanceGateEnabled {
        get { return _acceptanceGateEnabled; }
    }

    /// <summary>判定 AIRfc 是否已定义可验收断言（Scenarios / Assertions / 结构化 Items 任一非空）。</summary>
    public static bool AcceptanceDefined(AIRfc rfc) {
        if (rfc == null) {
            return false;
        }
        AIAcceptanceSpec a = rfc.Acceptance;
        if (a == null) {
            return false;
        }
        return !a.IsEmpty;
    }

    /// <summary>判定 AIRfc 是否已定义设计内容（Foresight/Convergence/Structure/Patterns/Rationale 任一非空）。</summary>
    public static bool DesignDefined(AIRfc rfc) {
        if (rfc == null) {
            return false;
        }
        AIDesignSpec d = rfc.Design;
        if (d == null) {
            return false;
        }
        return (d.Foresight != null && d.Foresight != "")
            || (d.Convergence != null && d.Convergence != "")
            || (d.Structure != null && d.Structure != "")
            || (d.Patterns != null && d.Patterns != "")
            || (d.Rationale != null && d.Rationale != "");
    }

    /// <summary>
    /// 追加需求澄清决策事件（airfc:clarify，场景 1.1 澄清向导）：记录追问与用户答复，
    /// 与 airfc:created/revised 并列进入决策轨迹（Agent 会话事件面）。
    /// </summary>
    public void RecordClarify(string question, string answer) {
        if (question == null) {
            question = "";
        }
        if (answer == null) {
            answer = "";
        }
        int rev = 0;
        if (_rfc != null) {
            rev = _rfc.Revision;
        }
        this.AppendDecision(AIDecisionEventKind.AirfcClarify, rev, "clarify: " + question + " → " + answer, "");
    }

    /// <summary>接线决策轨迹目标会话（RFC 038 §13 事件面）；之后 SetRfc/Checkpoint 等写入该会话。</summary>
    public void AttachSession(AISession session) {
        _session = session;
    }

    /// <summary>
    /// 设立初始 AIRfc 并记 airfc:created（Agent 会话事件面）。前置 RfcSpec 租约：冲突（他方
    /// 已持有同 RfcId 租约）→ 后到拒绝返回 null（不落变更、不追加事件）。
    /// </summary>
    public AIRfc? SetRfc(
        string rfcId,
        AIIntentionSpec intention,
        AIDesignSpec design,
        AIAcceptanceSpec acceptance) {
        AIRfc? rfc = _runtime.Create(rfcId, intention, design, acceptance, _holderId);
        if (rfc == null) {
            return null;
        }
        _rfc = rfc;
        this.AppendDecision(AIDecisionEventKind.AirfcCreated, rfc.Revision, rfc.ToContextBlock(), "");
        return rfc;
    }

    /// <summary>
    /// 纠偏：Spec 升版并记 airfc:revised（Agent 会话事件面）。前置 RfcSpec 租约：冲突 →
    /// 后到拒绝返回 null（不落变更、不追加事件）。B1 L2 检测：异来源覆盖同 acceptance 项 →
    /// 不落新 Revision，标 Contested + 登记冲突记录 + conflict:detected 事件，返回 Contested
    /// 版（调用方按 Status 区分正常升版与冲突升级）。
    /// </summary>
    public AIRfc? ReviseRfc(
        AIIntentionSpec? intention,
        AIDesignSpec? design,
        AIAcceptanceSpec? acceptance,
        string reason) {
        return this.ReviseRfc(intention, design, acceptance, reason, _holderId);
    }

    /// <summary>
    /// 纠偏升版（显式来源版）：语义同上，<paramref name="source"/> 为发起方（会话/分支）。
    /// 同来源修订 = 正常 refine；异来源覆盖同 acceptance 项 → L2 冲突升级。
    /// </summary>
    public AIRfc? ReviseRfc(
        AIIntentionSpec? intention,
        AIDesignSpec? design,
        AIAcceptanceSpec? acceptance,
        string reason,
        string source) {
        if (_rfc == null) {
            throw new ArgumentException("no active AIRfc");
        }
        // B1 L2 检测：字段级结构化 diff（AIAcceptanceSpec.Items 条目级比对）——异来源覆盖
        // 同 acceptance 项 → 反方向覆盖信号 → Contested + 冲突记录，不落新 Revision。
        // 仅对 Active 现行基线判矛盾（Rejected 基线已被否，异来源重入新方向不算矛盾）。
        AISpecConflict? conflict = null;
        if (_rfc.Status == AIRfcStatus.Active) {
            conflict = AISpecConflictDetector.Detect(
                _rfc, acceptance, source, _rfc.Source);
        }
        if (conflict != null) {
            AIRfc? contested = _runtime.MarkContested(_rfc.RfcId, conflict.Evidence);
            if (contested == null) {
                return null;
            }
            _rfc = contested;
            AIConflictRecord rec = _runtime.Conflicts.RecordSpecContradiction(
                conflict.RfcId,
                conflict.Revision,
                conflict.SourceA,
                conflict.SourceB,
                conflict.Resources,
                conflict.Evidence,
                conflict.BeforeAcceptance,
                conflict.AfterAcceptance);
            this.AppendDecision(
                AIDecisionEventKind.ConflictDetected,
                contested.Revision,
                "conflict:" + rec.ConflictId + " " + conflict.Evidence,
                reason != null ? reason : "");
            return contested;
        }
        int oldRev = _rfc.Revision;
        AIRfc? next = _runtime.Revise(_rfc.RfcId, intention, design, acceptance, reason, source);
        if (next == null) {
            return null;
        }
        _rfc = next;
        string detail = "v" + oldRev + " → v" + next.Revision + "\n" + next.ToContextBlock();
        this.AppendDecision(AIDecisionEventKind.AirfcRevised, next.Revision, detail, reason);
        this.BroadcastRevisionChanged(next, reason);
        return next;
    }

    /// <summary>
    /// A3 纠偏广播：ReviseRfc 成功后向在飞子代理广播 revision-changed 决策（广播 =
    /// TargetWorkItems 空；<see cref="AIParallelCoordinator.PendingSyncDecisionAsync"/>
    /// 处理：检查点 → 重建 ContextBlock → 租约重验 → 继续或 Failed）。无接线 → 不广播。
    /// </summary>
    private void BroadcastRevisionChanged(AIRfc next, string reason) {
        if (_rfcRevisionChanged == null || next == null) {
            return;
        }
        AISubAgentDecision decision = new AISubAgentDecision();
        decision.Kind = "revision-changed";
        decision.RfcId = next.RfcId;
        decision.RfcRevision = next.Revision;
        decision.TargetWorkItems = new List<string>();
        decision.Reason = reason != null ? reason : "";
        _rfcRevisionChanged(decision);
    }

    /// <summary>
    /// 拒绝当前 Active 版（Active → Rejected）并记 airfc:rejected（Agent 会话事件面）。
    /// 前置 RfcSpec 租约：冲突 → 后到拒绝返回 null（不落变更、不追加事件）。
    /// </summary>
    public AIRfc? RejectRfc(string reason) {
        if (_rfc == null) {
            throw new ArgumentException("no active AIRfc");
        }
        AIRfc? rejected = _runtime.RejectRfc(_rfc.RfcId);
        if (rejected == null) {
            return null;
        }
        this.AppendDecision(
            AIDecisionEventKind.AirfcRejected,
            rejected.Revision,
            rejected.ToContextBlock(),
            reason != null ? reason : "");
        return rejected;
    }

    /// <summary>
    /// 绑定 AIPlan 引用（Plan 面）。前置 Acceptance 先行门闩：启用且未定义验收断言
    /// （<see cref="AcceptanceDefined"/> 为 false）→ 抛 <see cref="ArgumentException"/>
    /// （禁先实现后补验收；先 /revise --acceptance= 补齐）。
    /// </summary>
    public AIRfc AttachPlan(AIPlan plan) {
        if (_rfc == null) {
            throw new ArgumentException("no active AIRfc");
        }
        if (_acceptanceGateEnabled && !AIHarnessSession.AcceptanceDefined(_rfc)) {
            throw new ArgumentException(
                "acceptance gate: AIRfc " + _rfc.RfcId + " v" + _rfc.Revision
                    + " 无 Acceptance 断言——先用 /revise --acceptance=<验收> 定义验收再 AttachPlan");
        }
        AIRfc rfc = _runtime.AttachPlan(_rfc.RfcId, plan);
        _rfc = rfc;
        return rfc;
    }

    /// <summary>
    /// 标记多来源需求冲突（Active → Contested，A.1；子代理管理方案 A 需求冲突）。仅 Active
    /// 生效。前置 RfcSpec 租约：冲突 → null（不落变更、不追加事件）。
    /// </summary>
    public AIRfc? ContendRfc(string reason) {
        if (_rfc == null) {
            throw new ArgumentException("no active AIRfc");
        }
        AIRfc? contested = _runtime.MarkContested(_rfc.RfcId, reason);
        if (contested != null) {
            _rfc = contested;
        }
        return contested;
    }

    /// <summary>
    /// 冲突解决（Contested → Active，A.1）：多来源需求收敛后恢复可修订面。仅 Contested 生效
    /// （非 Contested → null）。前置 RfcSpec 租约：冲突 → null。
    /// </summary>
    public AIRfc? ResolveRfc() {
        if (_rfc == null) {
            throw new ArgumentException("no active AIRfc");
        }
        AIRfc? resolved = _runtime.ResolveContested(_rfc.RfcId);
        if (resolved != null) {
            _rfc = resolved;
        }
        return resolved;
    }

    /// <summary>
    /// 人 CCB 裁决冲突（B1）：<see cref="AIConflictResolver.ResolveAsync"/> 唯一人入口——
    /// 记录 Open 且 resolvedBy 非空才生效。裁决后 Contested → Active 新 Revision 基线 +
    /// conflict:resolved / airfc:resolved 决策事件。返回新 Active 基线；未生效 → null。
    /// </summary>
    public AIRfc? ResolveConflictAsync(string conflictId, string decision, string reason, string resolvedBy) {
        AIConflictRecord? rec = _runtime.Conflicts.Find(conflictId);
        if (rec == null) {
            return null;
        }
        string rfcId = rec.RfcId;
        if (!_runtime.Conflicts.ResolveAsync(conflictId, decision, reason, resolvedBy)) {
            return null;
        }
        AIRfc? active = _runtime.GetActive(rfcId);
        int rev = 0;
        if (active != null) {
            _rfc = active;
            rev = active.Revision;
        }
        string decisionText = decision != null ? decision : "";
        this.AppendDecision(
            AIDecisionEventKind.ConflictResolved,
            rev,
            "conflict:" + conflictId + " decision:" + decisionText + " by:" + (resolvedBy != null ? resolvedBy : ""),
            reason != null ? reason : "");
        AIRfc? latest = active;
        if (latest != null) {
            this.AppendDecision(
                AIDecisionEventKind.AirfcResolved,
                latest.Revision,
                latest.ToContextBlock(),
                reason != null ? reason : "");
        }
        return active;
    }

    /// <summary>
    /// 拒绝冲突（B1）：<see cref="AIConflictResolver.RejectAsync"/> —— 冲突被否 →
    /// 记录 Rejected + AIRfc Contested → Rejected（方向被否，可 /revise 再入 Active）+
    /// conflict:rejected 决策事件。返回被拒版 AIRfc；未生效 → null。
    /// </summary>
    public AIRfc? RejectConflictAsync(string conflictId, string reason, string resolvedBy) {
        AIConflictRecord? rec = _runtime.Conflicts.Find(conflictId);
        if (rec == null) {
            return null;
        }
        string rfcId = rec.RfcId;
        if (!_runtime.Conflicts.RejectAsync(conflictId, reason, resolvedBy)) {
            return null;
        }
        AIRfc? latest = this.FindLatestFor(rfcId);
        int rev = 0;
        if (latest != null) {
            _rfc = latest;
            rev = latest.Revision;
        }
        this.AppendDecision(
            AIDecisionEventKind.ConflictRejected,
            rev,
            "conflict:" + conflictId + " rejected by:" + (resolvedBy != null ? resolvedBy : ""),
            reason != null ? reason : "");
        return latest;
    }

    /// <summary>取最后登记的版本（跨状态；供冲突拒绝后定位被拒版）。无 → null。</summary>
    private AIRfc? FindLatestFor(string rfcId) {
        if (rfcId == null || rfcId == "") {
            return null;
        }
        AIRfc? found = null;
        List<AIRfc> rfcs = _runtime.All();
        int i = 0;
        int n = rfcs.Count;
        while (i < n) {
            AIRfc r = rfcs[i];
            if (r != null && r.RfcId == rfcId) {
                found = r;
            }
            i = i + 1;
        }
        return found;
    }

    /// <summary>
    /// 进入冻结窗口（Active → Frozen，A.2）：冻结期间禁 Revise/Reject。仅 Active 生效。
    /// 前置 RfcSpec 租约：冲突 → null。
    /// </summary>
    public AIRfc? FreezeRfc(string reason) {
        if (_rfc == null) {
            throw new ArgumentException("no active AIRfc");
        }
        AIRfc? frozen = _runtime.FreezeRfc(_rfc.RfcId, reason);
        if (frozen != null) {
            _rfc = frozen;
        }
        return frozen;
    }

    /// <summary>
    /// 解冻（Frozen → Active，A.2）：冻结窗口结束恢复可修订面。仅 Frozen 生效。前置 RfcSpec
    /// 租约：冲突 → null。
    /// </summary>
    public AIRfc? UnfreezeRfc(string reason) {
        if (_rfc == null) {
            throw new ArgumentException("no active AIRfc");
        }
        AIRfc? unfrozen = _runtime.UnfreezeRfc(_rfc.RfcId, reason);
        if (unfrozen != null) {
            _rfc = unfrozen;
        }
        return unfrozen;
    }

    /// <summary>
    /// 收口关闭（Active/Frozen → Closed，D7 通过后禁再 Revise/Reject 的终态）并记
    /// airfc:closed 决策事件（Agent 会话事件面）。前置 RfcSpec 租约：冲突 → null（不落变更、
    /// 不追加事件）。
    /// </summary>
    public AIRfc? CloseRfc(string reason) {
        if (_rfc == null) {
            throw new ArgumentException("no active AIRfc");
        }
        AIRfc? closed = _runtime.CloseRfc(_rfc.RfcId, reason);
        if (closed == null) {
            return null;
        }
        _rfc = closed;
        this.AppendDecision(AIDecisionEventKind.AirfcClosed, closed.Revision, closed.ToContextBlock(), reason != null ? reason : "");
        return closed;
    }

    /// <summary>
    /// 撤单（Active/Frozen/Rejected → Cancelled，A.9 终态）并记 airfc:cancelled 决策事件。
    /// 前置 RfcSpec 租约：冲突 → null（不落变更、不追加事件）。
    /// </summary>
    public AIRfc? CancelRfc(string reason) {
        if (_rfc == null) {
            throw new ArgumentException("no active AIRfc");
        }
        AIRfc? cancelled = _runtime.CancelRfc(_rfc.RfcId, reason);
        if (cancelled == null) {
            return null;
        }
        _rfc = cancelled;
        this.AppendDecision(
            AIDecisionEventKind.AirfcCancelled,
            cancelled.Revision,
            cancelled.ToContextBlock(),
            reason != null ? reason : "");
        return cancelled;
    }

    /// <summary>
    /// 持久化 AIRfc 聚合根（含 Revision / Status / WorkItems / PlanId / Spec 三面）到
    /// <c>target/scratch/arcagent-state/airfc.json</c>（项目根相对；禁源码树）。无 AIRfc →
    /// false。AIPlan/门状态持久化登记次阶段（2.4 残余挂账）。
    /// </summary>
    public async Task<bool> SaveRfcAsync(CancellationToken cancellationToken) {
        if (_rfc == null || _project == "") {
            return false;
        }
        cancellationToken.ThrowIfCancellationRequested();
        string dir = Path.Combine(_project, AIHarnessSession.StateRelDir);
        if (!await this.EnsureDirectoryAsync(dir)) {
            return false;
        }
        string json = _runtime.Serialize();
        return await File.WriteAllTextAsync(Path.Combine(dir, AIHarnessSession.StateFile), json);
    }

    /// <summary>
    /// 从 <c>target/scratch/arcagent-state/airfc.json</c> 恢复 AIRfc 聚合根（非 transcript
    /// 重放冒充）：反序列化进运行时登记表并重建 <see cref="Rfc"/>（取恢复后的首个 Active 版）。
    /// 无文件 / 无 Active 版 → false。
    /// </summary>
    public async Task<bool> RestoreRfcAsync(CancellationToken cancellationToken) {
        if (_project == "") {
            return false;
        }
        cancellationToken.ThrowIfCancellationRequested();
        string path = Path.Combine(Path.Combine(_project, AIHarnessSession.StateRelDir), AIHarnessSession.StateFile);
        bool exists = await File.ExistsAsync(path);
        if (!exists) {
            return false;
        }
        string json = await File.ReadAllTextAsync(path);
        if (json == null || json == "") {
            return false;
        }
        if (!_runtime.Restore(json)) {
            return false;
        }
        AIRfc? active = _runtime.FirstActive();
        if (active == null) {
            return false;
        }
        _rfc = active;
        return true;
    }

    /// <summary>
    /// 持久化 AIPlan 树到 <c>target/scratch/arcagent-state/plan.json</c>（项目根相对；禁源码树）。
    /// 无计划 / 无计划门闩 / 无项目根 → false。门状态不持久化——下次 /dod 由
    /// <see cref="AIDoDOrchestrator.RunAutoGatesAsync"/> 实时重算（诚实标注「门状态重跑对齐」）。
    /// </summary>
    public async Task<bool> SavePlanAsync(CancellationToken cancellationToken) {
        AIPlan plan = _planGate != null ? _planGate.GetPlan() : null;
        if (plan == null || _project == "") {
            return false;
        }
        cancellationToken.ThrowIfCancellationRequested();
        string dir = Path.Combine(_project, AIHarnessSession.StateRelDir);
        if (!await this.EnsureDirectoryAsync(dir)) {
            return false;
        }
        AIPlanState state = new AIPlanState();
        state.Plan = plan;
        string json = JsonSerializer.Serialize((IJsonSerializable)state);
        return await File.WriteAllTextAsync(Path.Combine(dir, AIHarnessSession.PlanFile), json);
    }

    /// <summary>
    /// 从 <c>target/scratch/arcagent-state/plan.json</c> 恢复 AIPlan 树（非 transcript 重放）：
    /// 反序列化进新 AIPlan → 写入计划门闩 provider → 回链 AIRfc（PlanId 匹配才 AttachPlan）。
    /// 门状态下次 /dod 重跑。无文件 / 无计划 / 未挂计划门闩 → false。
    /// </summary>
    public async Task<bool> RestorePlanAsync(CancellationToken cancellationToken) {
        if (_project == "") {
            return false;
        }
        cancellationToken.ThrowIfCancellationRequested();
        string path = Path.Combine(Path.Combine(_project, AIHarnessSession.StateRelDir), AIHarnessSession.PlanFile);
        bool exists = await File.ExistsAsync(path);
        if (!exists) {
            return false;
        }
        string json = await File.ReadAllTextAsync(path);
        if (json == null || json == "") {
            return false;
        }
        AIPlanState state = new AIPlanState();
        JsonSerializer.Deserialize(json, (IJsonDeserializable)state);
        AIPlan plan = state.Plan;
        if (plan == null) {
            return false;
        }
        if (_planGate != null && _planGate.Provider != null) {
            _planGate.Provider.SetPlan(plan);
        }
        // 回链 AIRfc：PlanId 匹配才 AttachPlan（运行句柄重建）。
        if (_rfc != null && _rfc.PlanId != null && _rfc.PlanId != "" && plan.Id == _rfc.PlanId) {
            _rfc = _runtime.AttachPlan(_rfc.RfcId, plan);
        }
        return true;
    }

    /// <summary>项目根解析：文件路径取其父目录，否则原样（与 AICheckpointStore 同源）。</summary>
    private static string ResolveRoot(string project) {
        string target = project != null && project != "" ? project : ".";
        string root = target;
        if (File.Exists(target)) {
            string parent = Path.GetDirectoryName(target);
            root = parent != null && parent != "" ? parent : ".";
        }
        return root;
    }

    /// <summary>逐层创建目录（rt_dir_create 仅建单层，父目录缺失时递归创建）。</summary>
    private async Task<bool> EnsureDirectoryAsync(string dir) {
        bool exists = await Directory.ExistsAsync(dir);
        if (exists) {
            return true;
        }
        string parent = Path.GetDirectoryName(dir);
        if (parent != null && parent != "" && parent != dir) {
            bool okParent = await this.EnsureDirectoryAsync(parent);
            if (!okParent) {
                return false;
            }
        }
        return await Directory.CreateDirectoryAsync(dir);
    }

    /// <summary>
    /// 记录绿点：捕获真实工作区快照（git HEAD + stash 列表 + 文件清单 → 多绿点历史
    /// <c>target/scratch/arc-checkpoints/</c>：index.json + checkpoint-&lt;seq&gt;.json +
    /// objects/&lt;sha256&gt;.bin 大文件副本）并记 checkpoint:green 事件。快照内嵌当前 AIRfc
    /// Revision 与 AIPlan 状态摘要，供回滚联动恢复。返回是否成功捕获快照（项目根不可解析 →
    /// false 且事件 Detail 标注 snapshot:none）。
    /// </summary>
    public async Task<bool> CheckpointGreenAsync(string label, CancellationToken cancellationToken) {
        int rev = 0;
        if (_rfc != null) {
            rev = _rfc.Revision;
        }
        string l = label != null ? label : "";
        string planStatus = this.CurrentPlanStatusName();
        bool captured = await _checkpoints.CaptureAsync(l, rev, planStatus, cancellationToken);
        this.AppendDecision(
            AIDecisionEventKind.CheckpointGreen,
            rev,
            l + (captured ? " cp:" + _checkpoints.LatestCheckpointId + " snapshot:" + _checkpoints.StoreDir : " (snapshot:none)"),
            "");
        return captured;
    }

    /// <summary>
    /// 回滚最近绿点：按最近绿点快照执行真实文件回滚（恢复差异文件 + 删除新建文件；大文件经
    /// 内容寻址副本恢复），成功后记 checkpoint:rollback 事件。无快照 → 返回 false（升级人）。
    /// 事件 Detail 折叠回滚结果（含绿点 id / 版本）。
    /// </summary>
    public async Task<bool> CheckpointRollbackAsync(string label, string reason, CancellationToken cancellationToken) {
        return await this.CheckpointRollbackAsync(null, label, reason, cancellationToken);
    }

    /// <summary>
    /// 回滚到指定绿点（<paramref name="checkpointId"/> 为空 → 最近；场景 3.4 多绿点历史）：
    /// 真实文件回滚 + 联动恢复 AIRfc / AIPlan（若可重置）——快照内嵌 RfcRevision / PlanStatus
    /// 摘要，回滚后 AIRfc 版本恢复到绿点时点、AIPlan 状态复位（门状态无持久化面，/dod 重跑
    /// 时自然重对齐）。无快照 / 目标不存在 → 返回 false（升级人）。
    /// </summary>
    public async Task<bool> CheckpointRollbackAsync(string? checkpointId, string label, string reason, CancellationToken cancellationToken) {
        int rev = 0;
        if (_rfc != null) {
            rev = _rfc.Revision;
        }
        string l = label != null ? label : "";
        AICheckpointRollbackOutcome outcome = await _checkpoints.RollbackAsync(checkpointId, cancellationToken);
        string detail = l + " " + outcome.Describe();
        if (outcome.FoundSnapshot) {
            // 联动：AIRfc Revision 恢复（回滚目标为绿点记录的版本；不可恢复 → 诚实标注）。
            if (_rfc != null && outcome.RfcRevision > 0) {
                AIRfc? restored = _runtime.RestoreRevision(_rfc.RfcId, outcome.RfcRevision);
                if (restored != null) {
                    _rfc = restored;
                    detail = detail + " airfc:restored:v" + restored.Revision;
                } else {
                    detail = detail + " airfc:restore:none";
                }
            } else if (_rfc != null) {
                detail = detail + " airfc:no-revision";
            } else {
                detail = detail + " airfc:no-active";
            }
            // 联动：AIPlan 状态恢复（快照摘要可解析 → 复位；否则诚实标注未恢复）。
            if (_planGate != null && _planGate.GetPlan() != null) {
                AIPlan plan = _planGate.GetPlan();
                AIPlanStatus status = AIPlanStatus.Pending;
                bool parsed = AIHarnessSession.TryParsePlanStatus(outcome.PlanStatusSummary, out status);
                if (!parsed) {
                    detail = detail + " plan:status-unknown";
                } else {
                    plan.RestoreStatus(status);
                    detail = detail + " plan:restored:" + outcome.PlanStatusSummary;
                }
            } else if (_planGate != null) {
                detail = detail + " plan:none";
            } else {
                detail = detail + " plan:no-gate";
            }
            // 门状态：无持久化面（每次 /dod 由 RunAutoGatesAsync 实时重算），随文件 + AIRfc
            // 恢复自然重对齐，下次 /dod 重跑即生效。
            detail = detail + " gates:re-run-on-next-dod";
        }
        this.AppendDecision(AIDecisionEventKind.CheckpointRollback, rev, detail, reason);
        return outcome.Success;
    }

    /// <summary>是否存在最近绿点快照（L2 迭代前置检查；无绿点 → 升级人并提示先 /checkpoint）。</summary>
    public bool HasGreenPoint() {
        return _checkpoints.HasSnapshot();
    }

    /// <summary>
    /// L2 自动迭代闭环（RFC 043 场景 2.3 / 4.3，maxRounds=3 默认重载）：D0–D3 失败 →
    /// 结构化回喂 → ≤maxRounds 轮修复（<see cref="IAIFixRoundProvider"/>，REPL 以模型回合实现）→
    /// 全绿 Passed（携带轮数）；超限 → <see cref="CheckpointRollbackAsync"/> 回滚最近绿点 +
    /// 升级人（返回 NeedsHuman，<see cref="AIDoDFixLoopResult.RolledBack"/> 标注回滚结果）。
    /// 迭代前无绿点 → 返回 NeedsHuman（提示先 /checkpoint 打点），不烧迭代预算。
    /// </summary>
    public async Task<AIDoDFixLoopResult> RunFixLoopAsync(
        AIDoDGateKind gate,
        IAIFixRoundProvider? fixProvider,
        CancellationToken cancellationToken) {
        return await this.RunFixLoopAsync(gate, fixProvider, cancellationToken, 3);
    }

    /// <summary>L2 自动迭代闭环（显式 maxRounds 版）。语义同上。</summary>
    public async Task<AIDoDFixLoopResult> RunFixLoopAsync(
        AIDoDGateKind gate,
        IAIFixRoundProvider? fixProvider,
        CancellationToken cancellationToken,
        int maxRounds) {
        if (_rfc == null) {
            throw new ArgumentException("no active AIRfc — run /rfc first");
        }
        if (!_checkpoints.HasSnapshot()) {
            return AIDoDFixLoopResult.Escalated(
                gate,
                new List<AIDoDGateResult>(),
                null,
                0,
                "no green point before iteration — run /checkpoint first",
                false,
                false);
        }
        AIDoDFixLoopResult result = await _dod.RunFixLoopAsync(gate, _rfc, fixProvider, cancellationToken, maxRounds);
        if (result.BudgetExceeded) {
            bool rolled = await this.CheckpointRollbackAsync(
                "fix-loop-exceeded",
                "L2 iteration budget exceeded; rollback to last green point",
                cancellationToken);
            result.RolledBack = rolled;
            if (!rolled) {
                result.Reason = "fix budget exceeded and rollback failed — escalate to human";
            }
        }
        return result;
    }

    /// <summary>登记工作单元小结进决策轨迹（work_summary 事件）。</summary>
    public void RecordSummary(AIWorkSummary summary) {
        if (summary == null) {
            return;
        }
        int rev = 0;
        if (_rfc != null) {
            rev = _rfc.Revision;
        }
        this.AppendDecision(AIDecisionEventKind.WorkSummary, rev, summary.Format(), "");
    }

    /// <summary>
    /// M8 汇总门（既有签名）：跑 D0–D7 自动门，全 Passed（任一 Pending/Failed 即否——
    /// Pending ≠ Passed）才经 <see cref="AIPlanGate.CompleteByDoD"/> 受控 API 把计划转入
    /// Completed；通过时记 checkpoint:green 决策事件。D5/D7 人类门未确认（评估器须返回
    /// Passed 才放行；缺省语义见 <see cref="CompletePlanAfterDoDAsync(CancellationToken, bool, bool)"/>）。
    /// 计划不在 Verifying（步进未满额 / 已 Complete）或未挂载 PlanGate → 返回 false。
    /// </summary>
    public async Task<bool> CompletePlanAfterDoDAsync(CancellationToken cancellationToken) {
        return await this.CompletePlanAfterDoDAsync(cancellationToken, false, false);
    }

    /// <summary>
    /// M8 汇总门（D5/D7 人类门确认版）：D5 自审 / D7 人验收为人类门——REPL `/dod` 在
    /// D5 槽位证明齐全且 D7 一次人验收通过后以 <paramref name="d5Confirmed"/> /
    /// <paramref name="d7Confirmed"/> 确认调用；确认门按 Passed 计入，未确认门保留
    /// NeedsHuman（Pending ≠ Passed 语义不变）。全 Passed 才经受控 API 把计划转入 Completed；
    /// 通过时记 checkpoint:green 决策事件。计划不在 Verifying 或未挂载 PlanGate → 返回 false。
    /// </summary>
    public async Task<bool> CompletePlanAfterDoDAsync(
        CancellationToken cancellationToken,
        bool d5Confirmed,
        bool d7Confirmed) {
        if (_planGate == null || _rfc == null) {
            return false;
        }
        AIRfc rfc = _rfc;
        List<AIDoDGateResult> results = await _dod.RunAutoGatesAsync(rfc, cancellationToken);
        List<AIDoDGateResult> adjusted = AIHarnessSession.ApplyHumanGates(results, d5Confirmed, d7Confirmed);
        if (!AIDoDOrchestrator.AllPassed(adjusted)) {
            return false;
        }
        _planGate.CompleteByDoD();
        AIPlan plan = _planGate.GetPlan();
        if (plan == null || plan.Status != AIPlanStatus.Completed) {
            return false;
        }
        bool captured = await _checkpoints.CaptureAsync(
            "DoD D0-D7 all passed; plan completed", rfc.Revision, "Completed", cancellationToken);
        this.AppendDecision(
            AIDecisionEventKind.CheckpointGreen,
            rfc.Revision,
            "DoD D0-D7 all passed; plan completed" + (captured ? " cp:" + _checkpoints.LatestCheckpointId + " snapshot:" + _checkpoints.StoreDir : " (snapshot:none)"),
            "");
        return true;
    }

    /// <summary>把 D5/D7 人类门结果替换为 Passed（对应确认后）；其余结果原样保留。</summary>
    private static List<AIDoDGateResult> ApplyHumanGates(
        List<AIDoDGateResult> results,
        bool d5Confirmed,
        bool d7Confirmed) {
        List<AIDoDGateResult> outList = new List<AIDoDGateResult>();
        if (results == null) {
            return outList;
        }
        int i = 0;
        int n = results.Count;
        while (i < n) {
            AIDoDGateResult r = results[i];
            if (r != null) {
                if (d5Confirmed && r.Gate == AIDoDGateKind.D5SelfReview) {
                    outList.Add(AIDoDGateResult.Pass(AIDoDGateKind.D5SelfReview, r.Signal));
                } else if (d7Confirmed && r.Gate == AIDoDGateKind.D7HumanAccept) {
                    outList.Add(AIDoDGateResult.Pass(AIDoDGateKind.D7HumanAccept, r.Signal));
                } else {
                    outList.Add(r);
                }
            }
            i = i + 1;
        }
        return outList;
    }

    /// <summary>统一决策事件写入：折进 AIRfc 版本号，经 Agent 会话事件面 append（无会话则丢弃）。</summary>
    private void AppendDecision(AIDecisionEventKind kind, int revision, string detail, string reason) {
        if (_session == null) {
            return;
        }
        string d = "v" + revision + " " + detail;
        _session.AppendDecisionEvent(kind, d, reason);
    }

    /// <summary>当前绑定计划的 AIPlan 状态名称（无门闩/无计划 → 空串，绿点不记状态摘要）。</summary>
    private string CurrentPlanStatusName() {
        if (_planGate == null) {
            return "";
        }
        AIPlan plan = _planGate.GetPlan();
        if (plan == null) {
            return "";
        }
        return AIHarnessSession.PlanStatusName(plan.Status);
    }

    /// <summary>AIPlanStatus → 稳定名称（绿点快照 PlanStatus 摘要的 wire 面）。</summary>
    public static string PlanStatusName(AIPlanStatus status) {
        if (status == AIPlanStatus.Pending) { return "Pending"; }
        if (status == AIPlanStatus.Approved) { return "Approved"; }
        if (status == AIPlanStatus.Executing) { return "Executing"; }
        if (status == AIPlanStatus.Verifying) { return "Verifying"; }
        if (status == AIPlanStatus.Completed) { return "Completed"; }
        return "Rejected";
    }

    /// <summary>名称 → AIPlanStatus（绿点 PlanStatus 摘要回读；未知 → false）。</summary>
    public static bool TryParsePlanStatus(string text, out AIPlanStatus status) {
        if (text == "Pending") { status = AIPlanStatus.Pending; return true; }
        if (text == "Approved") { status = AIPlanStatus.Approved; return true; }
        if (text == "Executing") { status = AIPlanStatus.Executing; return true; }
        if (text == "Verifying") { status = AIPlanStatus.Verifying; return true; }
        if (text == "Completed") { status = AIPlanStatus.Completed; return true; }
        if (text == "Rejected") { status = AIPlanStatus.Rejected; return true; }
        status = AIPlanStatus.Pending;
        return false;
    }

    /// <summary>
    /// 把最新 AIRfc 折叠进 Instructions 后缀（方向环 → 执行环桥接）。
    /// 快照式接线：仅对宿主创建前组装好的选项生效（AIContextEngine 在 AIHost.Create 时
    /// 已把 Instructions 快照注册为 provider，运行中改字符串不会追入模型请求）。终端装配
    /// 请用活源 <see cref="AIRfcContextProvider"/>（组合根 AddProvider，/rfc /revise 后
    /// 锚点自动进请求、前缀稳定吃 KV cache），不要每轮重注入破坏前缀稳定。
    /// </summary>
    public void AttachRfcToInstructions(AISessionOptions options) {
        if (options == null || _rfc == null) {
            return;
        }
        string block = _rfc.ToContextBlock();
        if (options.Instructions == null || options.Instructions == "") {
            options.Instructions = block;
        } else {
            options.Instructions = options.Instructions + "\n\n" + block;
        }
    }
}
