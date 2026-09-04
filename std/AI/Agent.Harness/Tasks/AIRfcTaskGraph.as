// RFC 043 P3（parallel-subagents §3.2）：AIRfc.WorkItems 任务图 —— DependsOn 拓扑就绪面。
// 只消费 Airfc.WorkItems + DependsOn，不发明锁/队列/事件（写面冲突仲裁在 AIParallelCoordinator 走 AICoordinator）。
namespace Arc.Agent.Harness;
using Arc;
using Arc.Agent;
using Arc.Collections;

/// <summary>
/// 任务图：由 <see cref="AIRfc.WorkItems"/> + 各工作项 <see cref="AIRfcWorkItem.DependsOn"/>
/// 构建的拓扑就绪面。依赖全部 Done 才 Ready；派发前 MarkInProgress 占用；完成必带小结
/// （<see cref="MarkDone(string, AIWorkSummary)"/>，null 小结抛 ArgumentException）。
/// </summary>
public class AIRfcTaskGraph {
    private List<AIRfcWorkItem> _items;
    // 完成入账：工作项 id → 小结（MarkDone/MarkFailed/MarkCancelled 汇合点记录；无小结不得终态）。
    private List<string> _doneIds;
    private List<string> _failedIds;
    private List<string> _cancelledIds;
    private List<AIWorkSummary> _summaries;
    // 决策轨迹目标会话（可选）：MarkDone/MarkFailed/MarkCancelled 汇合点经 Agent 会话事件面追加
    // work_summary / airfc:cancelled（单轨，无独立日志库）。
    private AISession? _session;

    public AIRfcTaskGraph(List<AIRfcWorkItem> items) {
        _items = new List<AIRfcWorkItem>();
        _doneIds = new List<string>();
        _failedIds = new List<string>();
        _cancelledIds = new List<string>();
        _summaries = new List<AIWorkSummary>();
        if (items != null) {
            int i = 0;
            while (i < items.Count) {
                AIRfcWorkItem it = items[i];
                if (it != null) {
                    _items.Add(it);
                }
                i = i + 1;
            }
        }
        this.Validate();
    }

    /// <summary>
    /// 构造校验（parallel-subagents §3.1）：依赖引用不存在的工作项 Id 为非法 → ArgumentException；
    /// 重复 Id → ArgumentException。
    /// </summary>
    private void Validate() {
        int i = 0;
        while (i < _items.Count) {
            AIRfcWorkItem item = _items[i];
            int j = 0;
            while (j < _items.Count) {
                if (i != j && _items[j].WorkItemId == item.WorkItemId) {
                    throw new ArgumentException("duplicate work item id: " + item.WorkItemId);
                }
                j = j + 1;
            }
            if (item.DependsOn != null) {
                int k = 0;
                while (k < item.DependsOn.Count) {
                    string dep = item.DependsOn[k];
                    if (AIRfcWorkItem.FindItem(_items, dep) == null) {
                        throw new ArgumentException("work item " + item.WorkItemId + " depends on unknown id: " + dep);
                    }
                    k = k + 1;
                }
            }
            i = i + 1;
        }
    }

    /// <summary>全部工作项（构建时引用拷贝；状态随执行推进）。</summary>
    public List<AIRfcWorkItem> Items {
        get { return _items; }
    }

    /// <summary>
    /// A4 动态派发：运行中追加工作项。构造校验复用（重复 Id / 未知依赖即时拒绝）——
    /// 重复 Id 或依赖引用不存在的工作项 Id → false（不追加）；追加成功后该工作项按正常
    /// 就绪面参与后续派发（依赖未满保持 Blocked，依赖已满 Open，见 <see cref="Ready"/>）。
    /// </summary>
    public bool AttachItem(AIRfcWorkItem workItem) {
        if (workItem == null || workItem.WorkItemId == null || workItem.WorkItemId == "") {
            return false;
        }
        if (AIRfcWorkItem.FindItem(_items, workItem.WorkItemId) != null) {
            return false;
        }
        if (workItem.DependsOn != null) {
            int k = 0;
            while (k < workItem.DependsOn.Count) {
                string dep = workItem.DependsOn[k];
                if (dep != null && dep != "" && AIRfcWorkItem.FindItem(_items, dep) == null) {
                    return false;
                }
                k = k + 1;
            }
        }
        _items.Add(workItem);
        return true;
    }

    /// <summary>
    /// 拓扑就绪面：返回所有依赖已 Done 且自身未启动（非 InProgress / 非 Done / 非 Failed /
    /// 非 Cancelled）的工作项集。依赖未满的工作项置 <see cref="AIRfcWorkItemStatus.Blocked"/>
    /// （不入就绪面）；依赖已满且原为 Blocked 的工作项回到 Open（可派发）。Failed / Cancelled
    /// 工作项不进就绪面（终态不再执行）。
    /// </summary>
    public List<AIRfcWorkItem> Ready() {
        List<AIRfcWorkItem> ready = new List<AIRfcWorkItem>();
        int i = 0;
        while (i < _items.Count) {
            AIRfcWorkItem item = _items[i];
            if (item.Status == AIRfcWorkItemStatus.InProgress
                || item.Status == AIRfcWorkItemStatus.Done
                || item.Status == AIRfcWorkItemStatus.Failed
                || item.Status == AIRfcWorkItemStatus.Cancelled) {
                i = i + 1;
                continue;
            }
            if (item.HasUnfinishedDependency(_items)) {
                if (item.Status != AIRfcWorkItemStatus.Blocked) {
                    item.Status = AIRfcWorkItemStatus.Blocked;
                }
                i = i + 1;
                continue;
            }
            if (item.Status == AIRfcWorkItemStatus.Blocked) {
                item.Status = AIRfcWorkItemStatus.Open;
            }
            ready.Add(item);
            i = i + 1;
        }
        return ready;
    }

    /// <summary>
    /// 派发前占用：依赖未满 / 已占用 / 已完成 / 已失败 / 已取消 → false（重复占用拒绝）；成功 → InProgress。
    /// </summary>
    public bool MarkInProgress(string workItemId) {
        AIRfcWorkItem? item = AIRfcWorkItem.FindItem(_items, workItemId);
        if (item == null) {
            return false;
        }
        if (item.Status == AIRfcWorkItemStatus.InProgress
            || item.Status == AIRfcWorkItemStatus.Done
            || item.Status == AIRfcWorkItemStatus.Failed
            || item.Status == AIRfcWorkItemStatus.Cancelled) {
            return false;
        }
        if (item.HasUnfinishedDependency(_items)) {
            return false;
        }
        item.Status = AIRfcWorkItemStatus.InProgress;
        return true;
    }

    /// <summary>
    /// 完成入账（汇合点）：必带小结（<paramref name="summary"/> null → ArgumentException，
    /// 无小结不得 Done）；记录小结供汇总门 / 决策轨迹使用。已 Done / 已 Failed / 已 Cancelled
    /// → false（终态不翻转）。
    /// </summary>
    public bool MarkDone(string workItemId, AIWorkSummary summary) {
        if (summary == null) {
            throw new ArgumentException("MarkDone requires AIWorkSummary");
        }
        AIRfcWorkItem? item = AIRfcWorkItem.FindItem(_items, workItemId);
        if (item == null) {
            return false;
        }
        if (item.Status == AIRfcWorkItemStatus.Done
            || item.Status == AIRfcWorkItemStatus.Failed
            || item.Status == AIRfcWorkItemStatus.Cancelled) {
            return false;
        }
        item.Status = AIRfcWorkItemStatus.Done;
        _doneIds.Add(workItemId);
        _summaries.Add(summary);
        // 汇合点写决策事件（单轨：Agent 会话事件面；无会话则丢弃）。
        if (_session != null) {
            _session.AppendDecisionEvent(AIDecisionEventKind.WorkSummary, summary.Format());
        }
        return true;
    }

    /// <summary>
    /// 取消入账（A2 撤单收束）：必带小结（<paramref name="summary"/> null → ArgumentException，
    /// 无小结不得取消入账）；记录小结 + 写 airfc:cancelled 决策事件。已 Done / 已 Failed /
    /// 已 Cancelled → false。取消项不进就绪面、不计入 <see cref="HasRemaining"/>（撤单后不再
    /// 执行、不阻塞收束）。
    /// </summary>
    public bool MarkCancelled(string workItemId, AIWorkSummary summary) {
        if (summary == null) {
            throw new ArgumentException("MarkCancelled requires AIWorkSummary");
        }
        AIRfcWorkItem? item = AIRfcWorkItem.FindItem(_items, workItemId);
        if (item == null) {
            return false;
        }
        if (item.Status == AIRfcWorkItemStatus.Done
            || item.Status == AIRfcWorkItemStatus.Failed
            || item.Status == AIRfcWorkItemStatus.Cancelled) {
            return false;
        }
        item.Status = AIRfcWorkItemStatus.Cancelled;
        _cancelledIds.Add(workItemId);
        _summaries.Add(summary);
        // 汇合点写决策事件（单轨：Agent 会话事件面；无会话则丢弃）。
        if (_session != null) {
            _session.AppendDecisionEvent(AIDecisionEventKind.AirfcCancelled, summary.Format());
        }
        return true;
    }

    /// <summary>
    /// 失败入账（D1：失败信号持久承载，跨会话可查）：必带小结（<paramref name="summary"/> null
    /// → ArgumentException，无小结不得 Failed）；记录小结 + 写 work_summary 决策事件（失败必答
    /// 小结仍入轨迹）。已 Done / 已 Failed / 已 Cancelled → false。失败项不进就绪面、不计入
    /// <see cref="HasRemaining"/>（终态，不再执行、不阻塞收束）；依赖保持 Blocked。
    /// </summary>
    public bool MarkFailed(string workItemId, AIWorkSummary summary) {
        if (summary == null) {
            throw new ArgumentException("MarkFailed requires AIWorkSummary");
        }
        AIRfcWorkItem? item = AIRfcWorkItem.FindItem(_items, workItemId);
        if (item == null) {
            return false;
        }
        if (item.Status == AIRfcWorkItemStatus.Done
            || item.Status == AIRfcWorkItemStatus.Failed
            || item.Status == AIRfcWorkItemStatus.Cancelled) {
            return false;
        }
        item.Status = AIRfcWorkItemStatus.Failed;
        _failedIds.Add(workItemId);
        _summaries.Add(summary);
        // 汇合点写决策事件（单轨：Agent 会话事件面；无会话则丢弃）。
        if (_session != null) {
            _session.AppendDecisionEvent(AIDecisionEventKind.WorkSummary, summary.Format());
        }
        return true;
    }

    /// <summary>决策轨迹目标会话（可选）；MarkDone/MarkFailed/MarkCancelled 汇合点经它追加事件。</summary>
    public AISession? DecisionSession {
        get { return _session; }
        set { _session = value; }
    }

    /// <summary>
    /// 是否仍有未完结工作项（含未启动 / 进行中 / 受阻；Done / Failed / Cancelled 视为终结）。
    /// </summary>
    public bool HasRemaining {
        get {
            int i = 0;
            while (i < _items.Count) {
                if (_items[i].Status != AIRfcWorkItemStatus.Done
                    && _items[i].Status != AIRfcWorkItemStatus.Failed
                    && _items[i].Status != AIRfcWorkItemStatus.Cancelled) {
                    return true;
                }
                i = i + 1;
            }
            return false;
        }
    }

    /// <summary>取已完结（Done/Failed/Cancelled）工作项的小结；未终结 / 未知 id → null。</summary>
    public AIWorkSummary? SummaryOf(string workItemId) {
        int i = 0;
        while (i < _doneIds.Count) {
            if (_doneIds[i] == workItemId) {
                return _summaries[i];
            }
            i = i + 1;
        }
        int j = 0;
        while (j < _failedIds.Count) {
            if (_failedIds[j] == workItemId) {
                return _summaries[_doneIds.Count + j];
            }
            j = j + 1;
        }
        int k = 0;
        while (k < _cancelledIds.Count) {
            if (_cancelledIds[k] == workItemId) {
                return _summaries[_doneIds.Count + _failedIds.Count + k];
            }
            k = k + 1;
        }
        return null;
    }

    /// <summary>已 Done 的工作项 Id 集合（决策轨迹 / 汇总门可枚举）。</summary>
    public List<string> DoneIds {
        get { return _doneIds; }
    }

    /// <summary>已 Failed 的工作项 Id 集合（失败信号审计；跨会话可查）。</summary>
    public List<string> FailedIds {
        get { return _failedIds; }
    }

    /// <summary>已 Cancelled 的工作项 Id 集合（撤单收束审计）。</summary>
    public List<string> CancelledIds {
        get { return _cancelledIds; }
    }
}
