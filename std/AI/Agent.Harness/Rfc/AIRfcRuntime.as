// RFC 043 H-2c/M5：AIRfc 运行时 — Create / Revise / AttachPlan / BindWorkItem / RejectRfc。
// 写路径持 AILeaseKind.RfcSpec 租约（经 AICoordinator 仲裁，M5 接线）；不平行实现冲突锁。
// S0 增补：AIRfc 生命周期新态（Contested/Frozen/Closed/Cancelled）+ 聚合根序列化/恢复
//（AIRfc 持久化，2.4 续跑前提；AIPlan/门状态持久化登记次阶段）。
namespace Arc.Agent.Harness;
using Arc;
using Arc.Agent;
using Arc.Collections;
using Arc.Text.Json;

/// <summary>AIRfc 签名级运行时；RfcSpec 租约经 AICoordinator 仲裁（RFC 043 M5 / conflict-fabric）。</summary>
public class AIRfcRuntime {
    private List<AIRfc> _rfcs;
    // 冲突织物（RFC 038 §13 / 043 M5）：挂载统一租约协调器 + 持有者（会话 id）后，写路径
    // 先经 AICoordinator 获取 AILeaseKind.RfcSpec 租约；冲突（其它会话已持有同 RfcId）→
    // 后到拒绝（返回 null）。未挂载 = 不参与跨会话仲裁（进程内登记）。
    private AICoordinator _coordinator;
    private string _holderId;
    // L2 冲突仲裁（方案 B B1）：机器检测记录 → 人 CCB 裁决。挂本运行时（共享同一运行时的
    // 会话共享记录表）；L1 租约仍经 _coordinator，本类不做平行锁。
    private AIConflictResolver _conflicts;

    public AIRfcRuntime() {
        _rfcs = new List<AIRfc>();
        _coordinator = null;
        _holderId = "";
        _conflicts = new AIConflictResolver(this);
    }

    /// <summary>L2 冲突仲裁器（机器记录 + 人 CCB 裁决；与 L1 RfcSpec 租约正交）。</summary>
    public AIConflictResolver Conflicts {
        get { return _conflicts; }
    }

    /// <summary>
    /// 挂载冲突织物协调器（RFC 043 M5）：绑定 AICoordinator + 持有者（会话 id）。挂载后
    /// Create / Revise / BindWorkItem / RejectRfc 先经 <see cref="AILeaseKind.RfcSpec"/> 租约
    /// 仲裁，冲突 → 后到拒绝（返回 null，不落变更）；先到者持有租约直至 ReleaseSession。
    /// 未挂载 = 既有行为不变（进程内登记，不参与跨会话仲裁）。
    /// </summary>
    public void AttachCoordinator(AICoordinator coordinator, string holderId) {
        _coordinator = coordinator;
        _holderId = holderId != null ? holderId : "";
    }

    public AIRfc? Create(
        string rfcId,
        AIIntentionSpec intention,
        AIDesignSpec design,
        AIAcceptanceSpec acceptance) {
        return this.Create(rfcId, intention, design, acceptance, "");
    }

    /// <summary>
    /// 创建 AIRfc（Revision = 1）。前置持有 <see cref="AILeaseKind.RfcSpec"/> 租约：冲突
    /// （其它会话已持有同 RfcId 租约）→ 后到拒绝返回 null（不落变更、不追加事件）。
    /// 同运行时时 rfcId 已存在属程序错误，抛 <see cref="ArgumentException"/>。
    /// <paramref name="source"/> 为来源（会话/分支），L2 冲突判定按来源区分。
    /// </summary>
    public AIRfc? Create(
        string rfcId,
        AIIntentionSpec intention,
        AIDesignSpec design,
        AIAcceptanceSpec acceptance,
        string source) {
        if (rfcId == null || rfcId == "") {
            throw new ArgumentException("rfcId is empty");
        }
        if (this.Find(rfcId) != null) {
            throw new ArgumentException("rfcId already exists: " + rfcId);
        }
        if (!this.TryBeginWrite(rfcId)) {
            return null;
        }
        AIRfc rfc = new AIRfc();
        rfc.RfcId = rfcId;
        rfc.Revision = 1;
        rfc.Intention = intention != null ? intention : new AIIntentionSpec();
        rfc.Design = design != null ? design : new AIDesignSpec();
        rfc.Acceptance = acceptance != null ? acceptance : new AIAcceptanceSpec();
        rfc.Status = AIRfcStatus.Active;
        rfc.Source = source != null ? source : "";
        _rfcs.Add(rfc);
        return rfc;
    }

    public AIRfc? Revise(
        string rfcId,
        AIIntentionSpec? intention,
        AIDesignSpec? design,
        AIAcceptanceSpec? acceptance,
        string reason) {
        return this.Revise(rfcId, intention, design, acceptance, reason, "");
    }

    /// <summary>
    /// 纠偏升版：Spec 面增量更新 → Revision+1；Active 旧版标记 Superseded（Rejected 旧版保持
    /// Rejected 审计状态）。前置 <see cref="AILeaseKind.RfcSpec"/> 租约：冲突 → 后到拒绝返回
    /// null（不落变更）。Rejected 版可经新 Revision 再入 Active（airfc §4.2 合法边）。
    /// <paramref name="source"/> 为来源（会话/分支），写入 <see cref="AIRfc.Source"/> 供 L2
    /// 冲突判定（B1）。L2 冲突升级由调用方（AIHarnessSession）在升版前经
    /// <see cref="AISpecConflictDetector"/> 预判。
    /// </summary>
    public AIRfc? Revise(
        string rfcId,
        AIIntentionSpec? intention,
        AIDesignSpec? design,
        AIAcceptanceSpec? acceptance,
        string reason,
        string source) {
        AIRfc current = this.RequireMutable(rfcId);
        if (!this.TryBeginWrite(rfcId)) {
            return null;
        }
        if (current.Status == AIRfcStatus.Active) {
            current.Status = AIRfcStatus.Superseded;
        }
        AIRfc next = new AIRfc();
        next.RfcId = current.RfcId;
        next.Revision = current.Revision + 1;
        next.Intention = intention != null ? intention : current.Intention;
        next.Design = design != null ? design : current.Design;
        next.Acceptance = acceptance != null ? acceptance : current.Acceptance;
        next.PlanId = current.PlanId;
        next.Plan = current.Plan;
        next.WorkItems = current.WorkItems;
        next.Status = AIRfcStatus.Active;
        next.Source = source != null ? source : "";
        _rfcs.Add(next);
        return next;
    }

    /// <summary>绑定已有 AIPlan（Plan 面 = 引用，不拷贝步骤；PlanId = AIPlan.Id 稳定键）。</summary>
    public AIRfc AttachPlan(string rfcId, AIPlan plan) {
        AIRfc rfc = this.Require(rfcId);
        if (plan == null) {
            throw new ArgumentException("plan is null");
        }
        rfc.PlanId = plan.Id;
        rfc.Plan = plan;
        return rfc;
    }

    /// <summary>
    /// 登记/绑定工作项（可跨 Session；工作项列表变更）。前置 <see cref="AILeaseKind.RfcSpec"/>
    /// 租约：冲突 → 后到拒绝返回 null（不落变更）。
    /// </summary>
    public AIRfcWorkItem? BindWorkItem(
        string rfcId,
        string workItemId,
        string title,
        string? sessionId,
        string? taskRunId) {
        return this.BindWorkItem(rfcId, workItemId, title, sessionId, taskRunId, null, null);
    }

    /// <summary>
    /// 登记/绑定工作项（可跨 Session；工作项列表变更）——含任务图字段（DependsOn / Scope，
    /// RFC 043 P3）。前置 <see cref="AILeaseKind.RfcSpec"/> 租约：冲突 → 后到拒绝返回 null
    /// （不落变更）。
    /// </summary>
    public AIRfcWorkItem? BindWorkItem(
        string rfcId,
        string workItemId,
        string title,
        string? sessionId,
        string? taskRunId,
        List<string>? dependsOn,
        List<string>? scope) {
        AIRfc rfc = this.Require(rfcId);
        if (workItemId == null || workItemId == "") {
            throw new ArgumentException("workItemId is empty");
        }
        if (!this.TryBeginWrite(rfcId)) {
            return null;
        }
        AIRfcWorkItem item = new AIRfcWorkItem();
        item.WorkItemId = workItemId;
        item.RfcId = rfcId;
        item.Title = title != null ? title : "";
        item.SessionId = sessionId;
        item.TaskRunId = taskRunId;
        item.Status = AIRfcWorkItemStatus.Open;
        if (dependsOn != null) {
            int d = 0;
            while (d < dependsOn.Count) {
                item.DependsOn.Add(dependsOn[d]);
                d = d + 1;
            }
        }
        if (scope != null) {
            int s = 0;
            while (s < scope.Count) {
                item.Scope.Add(scope[s]);
                s = s + 1;
            }
        }
        if (rfc.WorkItems == null) {
            rfc.WorkItems = new List<AIRfcWorkItem>();
        }
        rfc.WorkItems.Add(item);
        return item;
    }

    /// <summary>
    /// 拒绝当前 Active 版（Active → Rejected；airfc §4.2 合法边）。前置
    /// <see cref="AILeaseKind.RfcSpec"/> 租约：冲突 → 后到拒绝返回 null（不落变更）。
    /// 被拒版保持 Rejected 审计状态；须新 Revision 再入 Active（经 <see cref="Revise"/>）。
    /// </summary>
    public AIRfc? RejectRfc(string rfcId) {
        AIRfc current = this.Require(rfcId);
        if (!this.TryBeginWrite(rfcId)) {
            return null;
        }
        current.Status = AIRfcStatus.Rejected;
        return current;
    }

    /// <summary>
    /// 标记多来源需求冲突（Active → Contested，A.1）：冲突期间禁修订/拒绝，须先
    /// <see cref="ResolveContested"/> 解冲突再回 Active。前置 RfcSpec 租约：冲突 → null。
    /// </summary>
    public AIRfc? MarkContested(string rfcId, string reason) {
        AIRfc current = this.Require(rfcId);
        if (!this.TryBeginWrite(rfcId)) {
            return null;
        }
        current.Status = AIRfcStatus.Contested;
        return current;
    }

    /// <summary>
    /// 冲突解决（Contested → Active，A.1）：多来源需求收敛后恢复可修订面。仅 Contested 生效
    /// （非 Contested → 返回 null，调用方诚实降级）。前置 RfcSpec 租约：冲突 → null。
    /// </summary>
    public AIRfc? ResolveContested(string rfcId) {
        AIRfc? latest = this.FindLatest(rfcId);
        if (latest == null || latest.Status != AIRfcStatus.Contested) {
            return null;
        }
        if (!this.TryBeginWrite(rfcId)) {
            return null;
        }
        latest.Status = AIRfcStatus.Active;
        return latest;
    }

    /// <summary>
    /// 冲突裁决 → 新 Revision 基线（B1，人 CCB 裁决落点）：Contested 版转 Superseded，以
    /// <paramref name="winner"/> 为 Acceptance 新建 Active Revision+1（来源 = 裁决人）。仅
    /// Contested 生效；未持有 RfcSpec 租约 → null。与 <see cref="ResolveContested"/>（原地
    /// 翻转，S0 手动路径）正交——本方法写新基线，供 <see cref="AIConflictResolver.ResolveAsync"/>
    /// 消费。
    /// </summary>
    public AIRfc? ResolveContestedWithSpec(string rfcId, AIAcceptanceSpec winner, string resolvedBy) {
        AIRfc? latest = this.FindLatest(rfcId);
        if (latest == null || latest.Status != AIRfcStatus.Contested) {
            return null;
        }
        if (winner == null) {
            return null;
        }
        if (!this.TryBeginWrite(rfcId)) {
            return null;
        }
        latest.Status = AIRfcStatus.Superseded;
        AIRfc next = new AIRfc();
        next.RfcId = latest.RfcId;
        next.Revision = latest.Revision + 1;
        next.Intention = latest.Intention;
        next.Design = latest.Design;
        next.Acceptance = winner;
        next.PlanId = latest.PlanId;
        next.Plan = latest.Plan;
        next.Source = resolvedBy != null ? resolvedBy : "";
        next.Status = AIRfcStatus.Active;
        // 工作项列表新开一份（避免与旧版共享可变列表引用；旧版只读审计不受后续变更影响）。
        int w = 0;
        int wn = latest.WorkItems.Count;
        while (w < wn) {
            next.WorkItems.Add(latest.WorkItems[w]);
            w = w + 1;
        }
        _rfcs.Add(next);
        return next;
    }

    /// <summary>
    /// 冲突拒绝（B1，AIConflictResolver.RejectAsync 落点）：Contested → Rejected（方向被否，
    /// 须新 Revision 再入 Active）。仅 Contested 生效；未持有 RfcSpec 租约 → false。
    /// </summary>
    public bool RejectContested(string rfcId) {
        AIRfc? latest = this.FindLatest(rfcId);
        if (latest == null || latest.Status != AIRfcStatus.Contested) {
            return false;
        }
        if (!this.TryBeginWrite(rfcId)) {
            return false;
        }
        latest.Status = AIRfcStatus.Rejected;
        return true;
    }

    /// <summary>
    /// 进入冻结窗口（Active → Frozen，A.2）：冻结期间禁 Revise/Reject；可经
    /// <see cref="UnfreezeRfc"/> 解冻回 Active 或 <see cref="CloseRfc"/> 收口。仅 Active
    /// 生效。前置 RfcSpec 租约：冲突 → null。
    /// </summary>
    public AIRfc? FreezeRfc(string rfcId, string reason) {
        AIRfc current = this.Require(rfcId);
        if (!this.TryBeginWrite(rfcId)) {
            return null;
        }
        current.Status = AIRfcStatus.Frozen;
        return current;
    }

    /// <summary>
    /// 解冻（Frozen → Active，A.2）：冻结窗口结束恢复可修订面。仅 Frozen 生效。前置
    /// RfcSpec 租约：冲突 → null。
    /// </summary>
    public AIRfc? UnfreezeRfc(string rfcId, string reason) {
        AIRfc? latest = this.FindLatest(rfcId);
        if (latest == null || latest.Status != AIRfcStatus.Frozen) {
            return null;
        }
        if (!this.TryBeginWrite(rfcId)) {
            return null;
        }
        latest.Status = AIRfcStatus.Active;
        return latest;
    }

    /// <summary>
    /// 收口关闭（Active/Frozen → Closed，D7 通过后禁再 Revise/Reject 的终态）。仅
    /// Active/Frozen 生效（Contested 须先解冲突，Rejected 须先新 Revision）。前置 RfcSpec
    /// 租约：冲突 → null。
    /// </summary>
    public AIRfc? CloseRfc(string rfcId, string reason) {
        AIRfc? latest = this.FindLatest(rfcId);
        if (latest == null) {
            return null;
        }
        if (latest.Status != AIRfcStatus.Active && latest.Status != AIRfcStatus.Frozen) {
            return null;
        }
        if (!this.TryBeginWrite(rfcId)) {
            return null;
        }
        latest.Status = AIRfcStatus.Closed;
        return latest;
    }

    /// <summary>
    /// 撤单（Active/Frozen/Rejected → Cancelled，A.9 终态）：需求被撤回，只读。仅
    /// Active/Frozen/Rejected 生效。前置 RfcSpec 租约：冲突 → null。
    /// </summary>
    public AIRfc? CancelRfc(string rfcId, string reason) {
        AIRfc? latest = this.FindLatest(rfcId);
        if (latest == null) {
            return null;
        }
        if (latest.Status != AIRfcStatus.Active
            && latest.Status != AIRfcStatus.Frozen
            && latest.Status != AIRfcStatus.Rejected) {
            return null;
        }
        if (!this.TryBeginWrite(rfcId)) {
            return null;
        }
        latest.Status = AIRfcStatus.Cancelled;
        return latest;
    }

    /// <summary>
    /// 回滚联动（RFC 043 场景 3.4 推倒重来）：把 Active 方向回退到绿点记录的 Revision——
    /// 目标版（Superseded）转 Active、当前 Active 版转 Superseded。仅当目标版存在且为
    /// Superseded 时生效（Rejected 版不自动复活；目标不存在 / 与当前同版 → 返回 null，
    /// 调用方诚实降级）。与 <see cref="Revise"/>（升版）正交：回滚恢复旧版本号，不递增。
    /// 前置 <see cref="AILeaseKind.RfcSpec"/> 租约（与 Create/Revise 等写路径一致）：冲突 →
    /// 后到拒绝返回 null（不落变更）——回滚是 Spec 面写，不绕过租约。
    /// </summary>
    public AIRfc? RestoreRevision(string rfcId, int revision) {
        if (revision <= 0) {
            return null;
        }
        AIRfc? current = this.GetActive(rfcId);
        AIRfc? target = this.FindRevision(rfcId, revision);
        if (target == null || target == current) {
            return null;
        }
        if (target.Status != AIRfcStatus.Superseded) {
            return null;
        }
        if (!this.TryBeginWrite(rfcId)) {
            return null;
        }
        if (current != null && current.Status == AIRfcStatus.Active) {
            current.Status = AIRfcStatus.Superseded;
        }
        target.Status = AIRfcStatus.Active;
        return target;
    }

    /// <summary>取当前 Active 版；无则 null。</summary>
    public AIRfc? GetActive(string rfcId) {
        if (rfcId == null || rfcId == "") {
            return null;
        }
        AIRfc? found = null;
        int i = 0;
        int n = _rfcs.Count;
        while (i < n) {
            AIRfc r = _rfcs[i];
            if (r != null && r.RfcId == rfcId && r.Status == AIRfcStatus.Active) {
                found = r;
            }
            i = i + 1;
        }
        return found;
    }

    /// <summary>取登记的首个 Active 聚合根（跨会话恢复后重建 <c>AIHarnessSession.Rfc</c> 用）；无则 null。</summary>
    public AIRfc? FirstActive() {
        int i = 0;
        int n = _rfcs.Count;
        while (i < n) {
            AIRfc r = _rfcs[i];
            if (r != null && r.Status == AIRfcStatus.Active) {
                return r;
            }
            i = i + 1;
        }
        return null;
    }

    /// <summary>全部登记版（跨状态：Active/Superseded/Rejected/Contested/Frozen/Closed/Cancelled；审计面）。</summary>
    public List<AIRfc> All() {
        List<AIRfc> outList = new List<AIRfc>();
        int i = 0;
        int n = _rfcs.Count;
        while (i < n) {
            if (_rfcs[i] != null) {
                outList.Add(_rfcs[i]);
            }
            i = i + 1;
        }
        return outList;
    }

    /// <summary>
    /// 序列化全部 AIRfc（含 Revision / Status / WorkItems / PlanId / Spec 三面）为 JSON
    /// 文本（AIRfc 持久化，2.4 续跑前提；AIPlan/门状态持久化登记次阶段）。
    /// </summary>
    public string Serialize() {
        AIRfcState state = new AIRfcState();
        int i = 0;
        while (i < _rfcs.Count) {
            state.Rfcs.Add(_rfcs[i]);
            i = i + 1;
        }
        return JsonSerializer.Serialize((IJsonSerializable)state);
    }

    /// <summary>
    /// 反序列化覆盖登记表（同进程重放/跨会话恢复）。运行句柄（<see cref="AIRfc.Plan"/>）不
    /// 落盘，恢复后置 null——以 PlanId 为准，避免陈旧句柄冒充事实源。空 JSON → false。
    /// </summary>
    public bool Restore(string json) {
        if (json == null || json == "") {
            return false;
        }
        AIRfcState state = new AIRfcState();
        JsonSerializer.Deserialize(json, (IJsonDeserializable)state);
        _rfcs.Clear();
        if (state.Rfcs != null) {
            int i = 0;
            while (i < state.Rfcs.Count) {
                AIRfc rfc = state.Rfcs[i];
                if (rfc != null) {
                    rfc.Plan = null;
                    _rfcs.Add(rfc);
                }
                i = i + 1;
            }
        }
        return true;
    }

    private AIRfc Require(string rfcId) {
        AIRfc? r = this.GetActive(rfcId);
        if (r == null) {
            throw new ArgumentException("AIRfc not found or not Active: " + rfcId);
        }
        return r;
    }

    /// <summary>
    /// 取可修订的当前版：Active 优先，其次 Rejected（airfc §4.2：被拒版须新 Revision 再入
    /// Active）；Superseded 只读，禁止再修订。
    /// </summary>
    private AIRfc RequireMutable(string rfcId) {
        AIRfc? current = this.GetActive(rfcId);
        if (current == null) {
            current = this.FindRejected(rfcId);
        }
        if (current == null) {
            throw new ArgumentException("AIRfc not found or not mutable (Active/Rejected): " + rfcId);
        }
        return current;
    }

    private AIRfc? FindRejected(string rfcId) {
        AIRfc? found = null;
        int i = 0;
        int n = _rfcs.Count;
        while (i < n) {
            AIRfc r = _rfcs[i];
            if (r != null && r.RfcId == rfcId && r.Status == AIRfcStatus.Rejected) {
                found = r;
            }
            i = i + 1;
        }
        return found;
    }

    /// <summary>
    /// 写路径统一前置（RFC 038 §13 / 043 M5）：先经 <see cref="AICoordinator.Acquire"/> 取
    /// RfcSpec 租约，再经 <see cref="AICoordinator.CommitRfcSpec"/> 确权仍持有（Commit 不自动
    /// 放锁；编辑间隙保护）。未挂载协调器 → 放行（进程内登记）。冲突 → false（后到拒绝）。
    /// </summary>
    private bool TryBeginWrite(string rfcId) {
        if (_coordinator == null) {
            return true;
        }
        string id = rfcId != null ? rfcId : "";
        AILeaseKey key = AILeaseKey.RfcSpec(id);
        AIResourceGrant grant = _coordinator.Acquire(_holderId, key);
        if (grant == null || !grant.Acquired) {
            return false;
        }
        return _coordinator.CommitRfcSpec(_holderId, key);
    }

    private AIRfc? Find(string rfcId) {
        int i = 0;
        int n = _rfcs.Count;
        while (i < n) {
            AIRfc r = _rfcs[i];
            if (r != null && r.RfcId == rfcId) {
                return r;
            }
            i = i + 1;
        }
        return null;
    }

    /// <summary>取指定 RfcId 最后登记的版本（跨状态：Active/Superseded/Rejected/Contested/Frozen/Closed/Cancelled）。</summary>
    private AIRfc? FindLatest(string rfcId) {
        AIRfc? found = null;
        int i = 0;
        int n = _rfcs.Count;
        while (i < n) {
            AIRfc r = _rfcs[i];
            if (r != null && r.RfcId == rfcId) {
                found = r;
            }
            i = i + 1;
        }
        return found;
    }

    private AIRfc? FindRevision(string rfcId, int revision) {
        int i = 0;
        int n = _rfcs.Count;
        while (i < n) {
            AIRfc r = _rfcs[i];
            if (r != null && r.RfcId == rfcId && r.Revision == revision) {
                return r;
            }
            i = i + 1;
        }
        return null;
    }
}
