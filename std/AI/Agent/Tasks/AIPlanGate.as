// RFC 038 M8.2：AIPlanGate — 计划门闩（写拦截）+ 审批事件（框架内置）。
//
// 对齐 claude-code Plan Mode / Reasonix /plan：计划是一道门，未批准（Pending/Rejected）
// 时受约束的写入能力被拦截，错误对模型可见（提示等待审批/修订）。
//
// 框架内置 vs 应用侧策略的边界：
//   - 框架（本文件）：门闩判定（哪些能力受约束、计划是否放行）+ 审批状态机入口
//     （Approve/Reject）+ 计划生命周期事件回调（AIPlanApprovalHandler）——应用订阅事件
//     写会话日志/审计；内置 plan / mark_step_done / revise_plan 工具经 InstallTools 装配。
//   - 应用侧：声明受约束能力白名单（Options.PlanGatedCapabilities）+ 审批界面（订阅事件
//     或直接调 Approve/Reject）+ 方法论提示词（Instructions）。
//
// 拦截层级：调度层统一拦截（AIToolSandbox 经 Blocks 判定），工具作者零门槛、不会漏拦——
// 取代"每个写入工具手写 EnsureWritable"的散落式做法（根因解决而非截堵）。
//
// 门闩语义（Blocks）：仅当「能力 ∈ 受约束集合」且「存在未批准计划（Pending/Rejected）」
// 时拦截；只读能力 / 无计划（简单任务）一律放行。
namespace Arc.Agent;
using Arc;

/// <summary>计划生命周期事件回调（应用订阅：写会话日志 / 审计 / UI 通知）。</summary>
public class AIPlanApprovalHandler {
    /// <summary>计划创建（模型经 plan 工具）。</summary>
    public Action<AIPlan> OnPlanCreated;
    /// <summary>计划修订（模型经 revise_plan 工具）。</summary>
    public Action<AIPlan> OnPlanRevised;
    /// <summary>计划被批准（人类 /approve）。</summary>
    public Action<AIPlan> OnPlanApproved;
    /// <summary>计划被拒绝（人类 /reject）。</summary>
    public Action<AIPlan> OnPlanRejected;
    /// <summary>计划全部步骤完成、进入待 DoD 判定态（模型经 mark_step_done 步进满额）。</summary>
    public Action<AIPlan> OnPlanVerifying;
    /// <summary>计划经 DoD D0–D7 全勾判定通过（唯一 Completed 写入路径）。</summary>
    public Action<AIPlan> OnPlanCompleted;

    public AIPlanApprovalHandler() {
        this.OnPlanCreated = null;
        this.OnPlanRevised = null;
        this.OnPlanApproved = null;
        this.OnPlanRejected = null;
        this.OnPlanVerifying = null;
        this.OnPlanCompleted = null;
    }
}

/// <summary>
/// 计划门闩（Host 级实例，随 AIHost 装配）：持有当前计划 provider + 受约束能力集合 +
/// 审批事件回调。经 <see cref="InstallTools"/> 注册内置计划工具；经
/// <see cref="Blocks"/> 供调度层统一拦截。
/// </summary>
public class AIPlanGate {
    private List<string> _gatedCapabilities;
    private AIPlanApprovalHandler _events;
    // 冲突织物（RFC 038 §13）：可选挂载统一租约协调器 + 持有者（会话）。挂载后计划突变
    // （SetPlan / RevisePlan / MarkStepDone）先经 AICoordinator 获取 AILeaseKind.Plan 租约；
    // 冲突（其它任务已持有同计划租约）→ 后到拒绝（对模型可见）。未挂载 = 不参与跨任务仲裁。
    // 与 PlanGate 正交：门闩管「未批准能否副作用」，租约管「谁可改这份计划」，禁互替。
    private AICoordinator _coordinator;
    private string _holderId;

    public AIPlanGate() {
        Provider = null;
        _gatedCapabilities = new List<string>();
        _events = null;
        _coordinator = null;
        _holderId = "";
    }

    /// <summary>当前计划上下文源（读/展示同源）。</summary>
    public AIPlanContextProvider Provider { get; set; }

    /// <summary>是否启用门闩（受约束能力集合非空）。</summary>
    public bool IsEnabled {
        get { return _gatedCapabilities.Count > 0; }
    }

    /// <summary>绑定当前计划 provider（AIHost 装配时调用）。</summary>
    public void Attach(AIPlanContextProvider provider) {
        Provider = provider;
    }

    /// <summary>
    /// 挂载冲突织物协调器（跨任务/跨会话仲裁）：绑定 AICoordinator + 持有者（会话 id）。
    /// 挂载后 SetPlan / RevisePlan / MarkStepDone 经 <see cref="AILeaseKind.Plan"/> 租约仲裁，
    /// 冲突 → 后到拒绝（不排队）；先到者持有租约直至 ReleaseSession。未挂载 = 既有行为不变。
    /// </summary>
    public void AttachCoordinator(AICoordinator coordinator, string holderId) {
        _coordinator = coordinator;
        _holderId = holderId != null ? holderId : "";
    }

    /// <summary>设置受计划门闩约束的能力白名单（如 fs.Write / shell.Run）。</summary>
    public void SetGatedCapabilities(List<string> capabilities) {
        _gatedCapabilities.Clear();
        if (capabilities != null) {
            int n = capabilities.Count;
            int i = 0;
            while (i < n) {
                _gatedCapabilities.Add(capabilities[i]);
                i = i + 1;
            }
        }
    }

    /// <summary>订阅计划生命周期事件（应用写日志 / 审计）。</summary>
    public void SetEvents(AIPlanApprovalHandler events) {
        _events = events;
    }

    /// <summary>当前是否有计划。</summary>
    public bool HasPlan {
        get { return Provider != null && Provider.HasPlan; }
    }

    /// <summary>读取当前计划；null = 无计划。</summary>
    public AIPlan GetPlan() {
        return Provider != null ? Provider.GetPlan() : null;
    }

    /// <summary>清除当前计划（新任务开始时调用）。</summary>
    public void ClearPlan() {
        if (Provider != null) {
            Provider.ClearPlan();
        }
    }

    /// <summary>
    /// 创建新计划（内置 plan 工具调用）：填充字段 → Pending → 入 provider → 触发 OnPlanCreated。
    /// 步骤为空返回 null（工具层转 InvalidArgs）。
    /// </summary>
    public AIPlan SetPlan(string goal, string analysis, List<string> steps, string verification) {
        AIPlan plan = new AIPlan();
        plan.Goal = goal != null ? goal : "";
        plan.Analysis = analysis != null ? analysis : "";
        plan.Verification = verification != null ? verification : "";
        if (steps != null) {
            int n = steps.Count;
            int i = 0;
            while (i < n) {
                AIPlanNode node = new AIPlanNode();
                node.Kind = AIPlanNodeKind.Leaf;
                node.Title = steps[i] != null ? steps[i] : "";
                node.ParentId = plan.Tree.Root.Id;
                plan.Tree.Root.Children.Add(node);
                i = i + 1;
            }
        }
        if (plan.TotalSteps == 0) {
            return null;
        }
        // 冲突织物（RFC 038 §13）：计划登记前先获 AILeaseKind.Plan 租约——跨任务对同计划并发
        // 创建 → 后到拒绝（返回 null）。租约持续持有至 ReleaseSession（Commit 不自动放锁）。
        if (!this.AcquirePlanLease(plan.Id)) {
            return null;
        }
        plan.Status = AIPlanStatus.Pending;
        if (Provider != null) {
            Provider.SetPlan(plan);
        }
        this.FireCreated(plan);
        return plan;
    }

    /// <summary>
    /// 修订计划（内置 revise_plan 工具调用）：基于现有计划生成修订版（Revision+1，状态回
    /// Pending 需重审），填充新字段后入 provider → 触发 OnPlanRevised。
    /// </summary>
    public AIPlan RevisePlan(string goal, string analysis, List<string> steps, string verification) {
        // 冲突织物：修订以现有计划为目标（其稳定 Id 为租约键）；另一任务已持有该计划租约
        // → 后到拒绝（返回 null）。
        AIPlan prev = this.GetPlan();
        AIPlan next = prev != null ? prev.CreateRevision() : new AIPlan();
        next.Goal = goal != null ? goal : "";
        next.Analysis = analysis != null ? analysis : "";
        next.Verification = verification != null ? verification : "";
        if (steps != null) {
            int n = steps.Count;
            int i = 0;
            while (i < n) {
                AIPlanNode node = new AIPlanNode();
                node.Kind = AIPlanNodeKind.Leaf;
                node.Title = steps[i] != null ? steps[i] : "";
                node.ParentId = next.Tree.Root.Id;
                next.Tree.Root.Children.Add(node);
                i = i + 1;
            }
        }
        if (next.TotalSteps == 0) {
            return null;
        }
        if (!this.AcquirePlanLease(next.Id)) {
            return null;
        }
        next.Status = AIPlanStatus.Pending;
        if (Provider != null) {
            Provider.SetPlan(next);
        }
        this.FireRevised(next);
        return next;
    }

    /// <summary>批准当前计划（人类 /approve）：状态机放行 + 触发 OnPlanApproved。返回人类可见结果。</summary>
    public string Approve() {
        AIPlan plan = this.GetPlan();
        if (plan == null) {
            return "no plan to approve — the model has not created a plan yet";
        }
        if (plan.Status == AIPlanStatus.Rejected) {
            return "plan is rejected — the model must produce a revised plan (revise_plan) before it can be approved";
        }
        if (plan.IsExecutable) {
            return "plan already approved";
        }
        plan.Approve();
        this.FireApproved(plan);
        return "plan approved — write tools are now allowed";
    }

    /// <summary>拒绝当前计划（人类 /reject）：状态机置 Rejected + 触发 OnPlanRejected。</summary>
    public string Reject() {
        AIPlan plan = this.GetPlan();
        if (plan == null) {
            return "no plan to reject";
        }
        plan.Reject();
        this.FireRejected(plan);
        return "plan rejected — the model must produce a revised plan before writes";
    }

    /// <summary>
    /// 标记步骤完成（内置 mark_step_done 工具调用；index 为 1-based 叶序）；满额只到 Verifying
    /// （待 DoD 判定）。内部按 index 解析节点 Id 后落 <see cref="AIPlan.MarkNodeDone"/>（树化入口）。
    /// </summary>
    public string MarkStepDone(int index) {
        AIPlan plan = this.GetPlan();
        if (plan == null) {
            return "mark_step_done: no plan exists — call plan first";
        }
        // 冲突织物：步进以现有计划为目标；另一任务已持有该计划租约 → 后到拒绝（对模型可见）。
        if (!this.AcquirePlanLease(plan.Id)) {
            return "mark_step_done: plan is held by another task — later-arriver rejected (AILeaseKind.Plan conflict)";
        }
        if (index < 1 || index > plan.TotalSteps) {
            return "mark_step_done: step index " + index + " out of range (1.." + plan.TotalSteps + ")";
        }
        AIPlanNode node = plan.Steps[index - 1];
        plan.MarkNodeDone(node.Id);
        string progress = plan.CompletedSteps + "/" + plan.TotalSteps;
        if (plan.Status == AIPlanStatus.Verifying) {
            this.FireVerifying(plan);
            return "step " + index + " done — all steps done, awaiting DoD verdict (D0–D7) before completion (" + progress + ")";
        }
        return "step " + index + " done — progress " + progress;
    }

    /// <summary>
    /// DoD 受控完成（RFC 038 §12 / M8）：Completed 唯一受控写入入口，由 Harness/DoD 汇总门在
    /// D0–D7 全勾后调用；计划须处于 Verifying（全部步骤已完成、待判定）。模型路径
    /// （mark_step_done 等）禁止直改 Completed。
    /// </summary>
    public string CompleteByDoD() {
        AIPlan plan = this.GetPlan();
        if (plan == null) {
            return "complete_plan: no plan exists — call plan first";
        }
        // 冲突织物：完成判定同样以现有计划为目标；另一任务已持有该计划租约 → 后到拒绝。
        if (!this.AcquirePlanLease(plan.Id)) {
            return "complete_plan: plan is held by another task — later-arriver rejected (AILeaseKind.Plan conflict)";
        }
        if (plan.Status == AIPlanStatus.Completed) {
            return "complete_plan: plan already completed";
        }
        if (plan.Status != AIPlanStatus.Verifying) {
            return "complete_plan: plan is not awaiting DoD verdict — all steps must be done before completion";
        }
        plan.Complete();
        this.FireCompleted(plan);
        return "plan completed — DoD D0–D7 passed";
    }

    /// <summary>
    /// 门闩判定（调度层统一调用）：能力 ∈ 受约束集合且存在未批准计划（Pending/Rejected）
    /// → true（拦截）。只读能力 / 未启用 / 无计划一律放行（简单任务不拦）。
    /// </summary>
    public bool Blocks(string capability) {
        if (!this.IsEnabled) {
            return false;
        }
        if (capability == null || capability == "") {
            return false;
        }
        bool gated = false;
        int n = _gatedCapabilities.Count;
        int i = 0;
        while (i < n) {
            if (_gatedCapabilities[i] == capability) {
                gated = true;
            }
            i = i + 1;
        }
        if (!gated) {
            return false;
        }
        AIPlan plan = this.GetPlan();
        if (plan == null) {
            return false;
        }
        return plan.Status == AIPlanStatus.Pending || plan.Status == AIPlanStatus.Rejected;
    }

    /// <summary>门闩写拦截（非调度层路径，如第三方工具直接调用）：被拦则抛错（对模型可见）。</summary>
    public void EnsureWritable(string capability) {
        if (this.Blocks(capability)) {
            throw new Exception("write blocked by plan gate: plan is PENDING APPROVAL — wait for the human to approve the plan before writing");
        }
    }

    /// <summary>装配内置计划工具（plan / mark_step_done / revise_plan）进目标工具集。</summary>
    public void InstallTools(AIToolSet tools) {
        AIPlanTools.Install(this, tools);
    }

    // ── 私有：冲突织物 Plan 租约 ──

    /// <summary>
    /// 获取 AILeaseKind.Plan 租约（RFC 038 §13）：未挂载协调器 → 放行（无跨任务仲裁）；
    /// 冲突（其它任务已持有同计划租约）→ 后到拒绝（返回 false，不排队）。
    /// AIPlan 以稳定 Id 为租约标识（与 AIRfc.AttachPlan 的 PlanId 引用同源，均走
    /// <see cref="AILeaseKey.Plan"/> 的 "plan:"+Id 键约定）。
    /// </summary>
    private bool AcquirePlanLease(string planId) {
        if (_coordinator == null) {
            return true;
        }
        string id = planId != null ? planId : "";
        AILeaseKey key = AILeaseKey.Plan(id);
        AIResourceGrant grant = _coordinator.Acquire(_holderId, key);
        return grant != null && grant.Acquired;
    }

    // ── 私有：事件触发（null 安全） ──

    private void FireCreated(AIPlan plan) {
        if (_events != null && _events.OnPlanCreated != null) {
            _events.OnPlanCreated(plan);
        }
    }

    private void FireRevised(AIPlan plan) {
        if (_events != null && _events.OnPlanRevised != null) {
            _events.OnPlanRevised(plan);
        }
    }

    private void FireApproved(AIPlan plan) {
        if (_events != null && _events.OnPlanApproved != null) {
            _events.OnPlanApproved(plan);
        }
    }

    private void FireRejected(AIPlan plan) {
        if (_events != null && _events.OnPlanRejected != null) {
            _events.OnPlanRejected(plan);
        }
    }

    private void FireVerifying(AIPlan plan) {
        if (_events != null && _events.OnPlanVerifying != null) {
            _events.OnPlanVerifying(plan);
        }
    }

    private void FireCompleted(AIPlan plan) {
        if (_events != null && _events.OnPlanCompleted != null) {
            _events.OnPlanCompleted(plan);
        }
    }
}
