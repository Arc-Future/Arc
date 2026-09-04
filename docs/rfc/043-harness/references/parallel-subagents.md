# 并行子代理（P3）

> 关联 [043 Coding Agent Harness 工程(../../043-harness.md)（§2 · §10 · 宣称纪律）· [AIRfc](airfc.md) · [冲突织物](conflict-fabric.md) · [可执行 DoD](definition-of-done.md)。本子项定义 **P3：并行子代理** 的方法学契约——把 `AIRfc.WorkItems` 演进为**任务图**（`DependsOn` + `Scope`），每个工作项由独立子代理容器（独立 `AISession`）执行；**唯一验收权威 = 汇总门**。能力面与诚实缺口见 §8。

## §0 宣称门闩

未满足下列全部项前，**禁止**并行派发子代理，也禁止宣称 P3「完成 / Completed / 终态 / 已收敛」：

| # | 门闩 | 未过时 |
|---|------|--------|
| 1 | 冲突织物 **RfcSpec 租约闭环**全绿（三 Kind 一表；`CommitRfcSpec` 确权；无第二套锁，见 [conflict-fabric](conflict-fabric.md)） | 禁并行派发 |
| 2 | 未过**汇总门**（合并后完整 D0–D7）禁止宣称 `Completed` | 禁 `AIPlan.Completed` |
| 3 | 禁第二套 Coordinator / 锁 / 事件（只消费宿主面，见 §5 B1） | 禁「多任务安全完成」宣称 |

写代码前另须过 [llm-gates](llm-gates.md) 与 043 开篇读前门闩。

## §1 目标

| # | 目标 |
|---|------|
| G1 | `AIRfc.WorkItems` 演进为**任务图**（`DependsOn` + `Scope`）；依赖就绪才可派发 |
| G2 | 每工作项一**子代理容器**：独立 `AISession` + 只读共享 AIRfc 快照 + 经租约写 Spec |
| G3 | **唯一验收权威 = 汇总门**：合并后完整 D0–D7 全勾，子代理不得自报终态 |
| G4 | 并行度 / 预算 / 成本受控（`MaxConcurrentSubAgents` 默认 2–3；`TotalBudget`） |

一句话：**并行只是把「验证闭环」拆到多个隔离容器，验收仍回到单一汇总门。**

## §2 非目标

| # | 非目标 |
|---|--------|
| N1 | 多线程并发驱动同一 `AIHost` / `AISession`（单线程宿主模型下为 UB；L2 级并行宿主须另立 RFC） |
| N2 | 租约排队 / 自旋等待（后到一律拒绝；依赖等待由任务图表达，不冒充协调） |
| N3 | 子代理级 DoD 双标（子代理只回 `AIWorkSummary`，不做领域终态判定） |
| N4 | 自动裁决合并冲突（冲突整体回滚 + 升级人；禁自动选胜者） |

## §3 类型契约

> 命名一律 `AI` 前缀；归属包以 [api-sketch](api-sketch.md) 分层为基准。以下为**设计契约**；各类型按「能力面（真）/ 能力面（部分，注诚实缺口）/ 设计态（诚实缺口，未实现）」标注。

### 3.1 `AIRfcWorkItem` 升版（airfc §3 · `Arc.Agent.Harness`）

在 [airfc §3](airfc.md) / [api-sketch §3](api-sketch.md) 现有字段之上**新增两字段**：

| 字段 | 类型契约 | 约束 |
|------|----------|------|
| `DependsOn` | `List<string>`（前置工作项 Id） | 空 = 无依赖；引用不存在的工作项 Id 为**非法** |
| `Scope` | `List<string>`（文件 / 路径 / 资源声明） | 合并冲突仲裁的预声明写面；空 = 无预声明 |

### 3.2 `AIRfcTaskGraph`（`Arc.Agent.Harness` 基座）

| 成员 | 契约 |
|------|------|
| `Ready()` | 返回所有依赖已 `Done` 且自身未启动的工作项集（拓扑就绪面） |
| `MarkInProgress(workItemId)` | 派发前占用；重复占用 → `false` |
| `MarkDone(workItemId, AIWorkSummary)` | 完成入账；**必带**小结（[work-summary](work-summary.md)） |
| `HasRemaining` | 是否仍有未完结工作项 |

### 3.3 `AIParallelCoordinator`（`Arc.Agent.Harness` 基座）

> **只消费** `Arc.Agent.AICoordinator`（冲突织物），**不发明锁 / 队列 / 事件**。A1 起 `DispatchAsync` 不再派发即预取租约（惰性租约门 `AISubAgentLeaseGate`）；`RunAllAsync` 为 reconcile 循环。

| 成员 | 契约 |
|------|------|
| `MaxConcurrentSubAgents` | 并行度上限；**默认 2–3** |
| `Dispatch(workItem, sessionFactory, ct)` | 为单个就绪工作项启动子代理容器（装配惰性租约门；不预取租约） |
| `RunAllAsync(graph, ct)` | **reconcile 循环**跑完任务图（每 tick：派发新 Ready → `RunStepAsync` 逐回合推进 → 心跳 Dead/时长熔断 → 终结同 tick 入账 + 即时释放租约 → 预算守护） |
| `TotalBudget` | 总成本预算（含汇总门开销）；**公式已实现** + **A1 步数预算守护最小版**（超限停止新派发 + 在飞强制收束 + 升级人）；完整版（token/时长收束梯）设计态（归方案 A A4） |

### 3.4 `AISubAgentRun`（`Arc.Agent` 语义，可继承 `AITaskRun`）

| 字段 | 契约 |
|------|------|
| `WorkItemId` | 所服务工作项 |
| `RfcRevision` | 启动时 AIRfc 只读快照的 Revision（写回冲突以它为基准） |
| `AIWorkSummary` | **必答**小结（五字段，见 [work-summary](work-summary.md)） |

### 3.5 `AIMergeTransaction`（`Arc.Agent.Harness` 基座）—— **设计态，未实现**

两阶段提交：**staging 全落 → 统一 Move**。当前全库无此类型/代码实现；首切片以「汇总门先验各子代理结果 + ToolPath 租约写面互斥」替代（见 §8 注记与未落地面①）。

| 阶段 | 契约 |
|------|------|
| Stage 1 | 各子代理产物先落 staging（`target/scratch/` 或等价隔离区），**不触共享写面** |
| Stage 2 | 全部 staging 就绪 → 统一 Move 到工作区；**任一冲突** → 整体回滚 + 升级人 + [绿点回滚](definition-of-done.md) |

### 3.6 汇总门：`AIDoDOrchestrator.RunAggregatedGatesAsync`

| 成员 | 契约 |
|------|------|
| `RunAggregatedGatesAsync(rfc, results, ct)` | **汇总门唯一权威**：对合并后工作区跑完整 D0–D7；`Completed` 唯一写入路径 |

## §4 协议

### 4.1 派发

- 只派发 `Ready()` 工作项；按 `MaxConcurrentSubAgents` 节流，不一次放行全部。
- **只读 / 隔离面可高扇出**（多个子代理并行读共享 AIRfc / 互不相交的 `Scope`）；**共享写面串行**（同一 `RfcSpec` / `ToolPath` 一次仅一租约持有者）。

### 4.2 子代理上下文

- **全局块前缀稳定**：共用需求 / 设计 / 验收摘要置于前缀，跨子代理复用 KV cache。
- **任务专属块**：本工作项的 `Scope`、`DependsOn` 摘要、目标文件清单。
- **AIRfc 只读快照**：子代理持有启动时 Revision 的只读快照，不直接写共享 AIRfc；**Spec 写回主会话走 `RfcSpec` 租约**（见 [conflict-fabric](conflict-fabric.md)）。

### 4.3 写冲突

- 子代理 `ToolPath` 写冲突：**后到拒绝**（`Acquired=false`）→ 该子代理 `Failed` + 小结上报；不静默降级、不自旋等待。

### 4.4 合并

- 子代理全部终结 → **汇总门完整 D0–D7**（对合并后总工作区）。
- **D4** 对**合并后总 diff** 判定覆盖 / 越界（非各子代理分片）。
- **D5** 合并自审：每条 acceptance 附可执行证明（跨子代理证据汇总）。
- **D7** **一次**人验收（不按子代理逐个重复打扰）。

### 4.5 成本

- `TotalBudget = Σ 各子代理 + 汇总门`（**仅公式已实现**：`AIParallelCoordinator.TotalBudget` 可算）；单子代理 / 总预算超限 → **强制收束**（取消未启动、回收已占用、升级人）——**未落地（设计态）**，禁止无界追加。

## §5 禁令

| # | 禁令 |
|---|------|
| B1 | 禁第二套 Coordinator / 锁 / 事件（只消费 `AICoordinator` 与 Agent 会话事件） |
| B2 | 禁子代理自报 Passed 冒充 Completed（终态唯一在汇总门） |
| B3 | 禁租约排队 / 自旋冒充依赖等待（依赖由任务图 `DependsOn` 表达） |
| B4 | 禁无界扇出（并行度受 `MaxConcurrentSubAgents` 约束） |
| B5 | 禁多线程并发驱动同一 `AIHost`（单线程宿主模型；L2 另 RFC） |
| B6 | 禁合并冲突自动选胜者（整体回滚 + 升级人） |

## §6 验收 DoD

| # | 验收项 | 通过信号 |
|---|--------|----------|
| D1 | 任务图边表可测 | `Ready()` 拓扑、非法依赖拒绝、`MarkDone` 推进有正反可执行用例（`arc_ai_parallel_dependency_e2e`） |
| D2 | 并行冲突可测 | 共享写面后到拒绝可复现、只读高扇出不互阻（`arc_ai_parallel_toolpath_conflict_e2e`：真实首写冲突——fs.Write 首次写时后到拒绝，Failed + 必答小结含 ToolPath 明细） |
| D3 | 合并事务可测 | 诚实缺口：`AIMergeTransaction` 未实现（设计态），无对应验收路径（staging 统一 Move + 冲突整体回滚待收束） |
| D4 | 汇总门唯一权威 | 子代理 Passed ≠ Completed；仅 `RunAggregatedGatesAsync` 全勾可终态；green / red 两路径均有可执行用例（`independent_scope` 绿 + `toolpath_conflict` 真实首写冲突红） |
| D5 | 成本受控可测 | `TotalBudget` 公式 + A1 步数预算守护最小版已具备（超限停止新派发 + 在飞强制收束 + 升级人）；诚实缺口：完整版 token/时长收束梯（方案 A A4）未实现 |
| D6 | 上下文分层可测 | 全局前缀 + 任务专属块已具备（`BuildTaskContext`）；诚实缺口：KV cache 复用可观测（设计态） |

## §7 上下游链接

| 方向 | 链接 | 关系 |
|------|------|------|
| 任务图数据面 | [airfc §3](airfc.md) · [api-sketch §3](api-sketch.md) | `AIRfcWorkItem` 升版（`DependsOn` / `Scope`） |
| 写冲突 | [conflict-fabric](conflict-fabric.md) · [038 §13](../../038-ai-host.md) | 只消费租约；禁第二套锁 |
| 验收 | [definition-of-done](definition-of-done.md) | 汇总门 D0–D7 唯一权威 |
| 小结 | [work-summary](work-summary.md) | 子代理必答小结 |
| 写代码门闩 | [llm-gates](llm-gates.md) | P3 开工前硬门闩 |
| 主文档 | [043(../../043-harness.md) | 宣称纪律 · 试探≠终态 |

## §8 能力面与诚实缺口

| 面 | 能力面（真） | 诚实缺口 / 设计态 |
|----|------|------|
| `AIRfcWorkItem` | P3 升版：`WorkItemId` / `RfcId` / `Title` / `SessionId` / `TaskRunId` / `Status`（Open/InProgress/Blocked/Done/**Cancelled**）+ **`DependsOn` / `Scope`**（§3.1；`AIRfcRuntime.BindWorkItem` 增依赖/写面重载） | — |
| Coordinator | 三 Kind 一表（`AILeaseKind` 三 Kind + `Acquire` / `Release` / `CommitRfcSpec`；RfcSpec 租约已接线）+ **A1 惰性租约门 `AISubAgentLeaseGate`**（装配到子代理会话 sandbox 调度层，首次真实写前逐个 `Acquire(ToolPath)`；同波工作项不再因预取互相误伤，真冲突——首次写时路径被占——仍后到拒绝 → 工作项 Failed + 小结上报）；AIRfc 写回走 RfcSpec 租约（`AttachRfcLease`）；**无第二套锁** | — |
| 汇总门 | `RunAggregatedGatesAsync(rfc, subAgents, ct)`（合并子代理结果先验 + 对合并后总工作区跑完整 D0–D7；任一 Failed/未完结/无小结 → 汇总门红，Pending≠Passed）；green / red 两路径均有可执行用例（`independent_scope` 绿、`toolpath_conflict` 真实首写冲突红） | — |
| 任务图 / 并行容器 | P3 首切片：`AIRfcTaskGraph`（Ready / MarkInProgress / MarkDone / MarkCancelled / HasRemaining / 非法依赖构造拒绝）/ `AIParallelCoordinator`（A1 reconcile 循环：每 tick 派发新 Ready → `RunStepAsync` 逐回合推进 → 心跳 Dead/时长熔断 → 终结同 tick 入账 + `ReleaseSession` 即时释放 → 预算守护；`MaxConcurrentSubAgents` 节流 + `TotalBudget`）/ `AISubAgentRun : AITaskRun`（WorkItemId / RfcRevision / 惰性租约门 / 必答小结）。可执行用例：`arc_ai_parallel_subagents_e2e` 5 用例——`dependency`（DependsOn 拓扑）、`independent_scope`（并行成功 + 汇总门绿）、`toolpath_conflict`（真实首写冲突后到 Failed + 汇总门红）、`lazy_lease_overlap`（A1 判别性：Scope 重叠但顺序写不同路径 → 不预取互伤）、`immediate_release`（A1 判别性：死亡/失败同 tick 即时释放租约，被占路径可被后续取得） | — |
| 状态 | A1：reconcile 循环化 + 租约惰性化 + 失败/死亡即时释放租约；同波预取假冲突消除。**A2**（生命周期状态机 + 撤单收束）已实现 | `TotalBudget` 超限强制收束完整版 / `AIMergeTransaction` 仍设计态 |

> **A1 能力面注记**：① 会话/子代理容器复用 `AITaskRun`/`AISession`（`AISubAgentRun` 继承 `AITaskRun`），`RunAllAsync` 为 **reconcile 循环**（每 tick 逐回合 `RunStepAsync` 推进，替代波内逐个 await 到完结；**逻辑并行 = async 交错**，单线程宿主不宣称多线程并发驱动同一 AIHost，B5）。② 惰性租约门写工具识别约定：capability 含 "Write"（对齐 `fs.Write` 命名）；非写能力 / 非声明写面一律放行（读取不阻塞写入）；流式 TakeOver 路径不经过 `ExecuteAsync` 租约门（非流式子代理会话为主，诚实边界）。③ 合并事务 `AIMergeTransaction` 两阶段提交未实现，以「汇总门先验各子代理结果 + ToolPath 租约写面互斥」替代（同路径后到拒绝，禁止并行写；无自动选胜者），升级人路径保留。

> **未落地面（诚实清单）**：
> ① **`AIMergeTransaction` 两阶段提交**（staging 全落 → 统一 Move + 冲突整体回滚）——设计态，未实现（§3.5）；
> ② **`TotalBudget` 超限强制收束完整版**——A1 具备**步数预算守护最小版**（reconcile 每 tick 检查，超限停止新派发 + 在飞强制收束 + 升级人）；完整版（token/时长维度收束梯，[subagent-management §6](subagent-management.md)）仍设计态（§3.3 / §4.5 / 方案 A A4）；
> ③ **KV cache 复用可观测**——`PrefixContext` 已接线但无观测面，设计态（§6 D6）；
> ④ **`AIPlan.Id`（P3 前置项）未引入**——Plan 租约键暂以 Goal 合成，见 plan.md SR-1；
> ⑤ **A2 撤单收束能力面**——生命周期状态机 + `CancelPendingAsync`：`AISubAgentState`（Pending/Spawned/Running/Interrupted/Paused/Completed/Failed/Cancelled/Dead，与 `AITaskRunStatus` 映射）+ `Spawn/Interrupt/Reap`；`RunAllAsync` 修取消被吞（不再无条件 `Complete()`，ct 取消/撤单 → Cancelled + 必答小结，汇总门按未完结红）；`CancelPendingAsync`（取消未启动 + 中断在飞 + 联动 CTS + 即时释放租约 + 回收 Dead）；`AIRfcTaskGraph.MarkCancelled`；`AIRfcWorkItemStatus.Cancelled` / `AIRfcStatus.Cancelled`；`airfc:cancelled` / `subagent:interrupt` 决策事件。可执行用例 `arc_ai_cancel_pending_e2e`（详见 [subagent-management A2](subagent-management.md) / plan.md A2 登记）

---

[返回 043(../../043-harness.md) · [AIRfc](airfc.md) · [冲突织物](conflict-fabric.md) · [references 索引](index.md)
