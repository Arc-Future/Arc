# 子代理管理（方案 A · AISubAgentManager）

> 关联 [043 Coding Agent Harness 工程(../../043-harness.md)（§2 · 宣称纪律）· [并行子代理（P3）](parallel-subagents.md)（现状底座）· [冲突织物](conflict-fabric.md) · [AIRfc](airfc.md) · [可执行 DoD](definition-of-done.md) · [真实场景运转协议](scenario-operation.md)（3.1 / 3.2 / 3.3 · A.9 · B11 · 场景 3.5）。本子项定义 **两大机制之方案 A：灵活成熟的子代理管理** 的设计契约——把 P3 的 `AIParallelCoordinator` 从「波次派发 + await 到完结」升级为 **Kubernetes 控制器式 reconcile 循环**，覆盖子代理全生命周期治理、监督与失败隔离、运行中消息与决策同步、动态派发与弹性、预算强制收束与结果汇总。
>
> **能力面与诚实缺口**：方案 A 的能力契约覆盖 A1–A5（见 §9）。已具备的能力面：reconcile 循环 `RunAllAsync` + 租约惰性化 + 失败/死亡即时释放（A1）；生命周期状态机 + 撤单收束 + 修取消被吞（A2）；决策广播 + 重对齐 + 租约重验 + 旁路注入（A3）；动态派发 + 弹性并行度 + `TotalBudget` 强制收束梯（A4）；成本核算 / 观测（`TotalUsage`/`TotalTurns`/`TotalBudgetExceeded`/`InFlightCount` + `subagent:usage` 事件，A5）。诚实缺口（设计态，未实现）：`AISubAgentManager` 更名 / `ResumeAsync`·`CancelAsync(runId)` / `AISupervisionPolicy`·`AISubAgentBudget` / graph `Add`·`Invalidate`·`Reprioritize` + `AIRfcWorkItem.Priority` / 汇总门增强，见 §10。
>
> **设计来源**：已完成的两大机制系统性设计 + 「运行中子代理干预」只读推导（6 细分场景：S-1 补充问题 / S-2 纠正需求 / S-3 停止子代理 / S-4 决策同步 / S-5 拉起新子代理 / S-6 多事件混合）。本子项是其 RFC 级结构化落盘，**未臆造新架构**。

## §0 宣称门闩

未满足下列全部项前，**禁止**宣称「子代理管理 / 方案 A 完成 / 已收敛 / 运行中干预可用」：

| # | 门闩 | 未过时 |
|---|------|--------|
| 1 | 底座能力面成立：P3 首切片（`AIRfcTaskGraph` / `AIParallelCoordinator` / `AISubAgentRun` / `RunAggregatedGatesAsync` 汇总门）（见 [parallel-subagents](parallel-subagents.md) §8） | 禁在空底座上宣称「治理完成」 |
| 2 | 本子项 §2–§8 的类型契约与 043 分层 / api-sketch 归属一致；命名一律 `AI` 前缀、归属 `Arc.Agent.Harness` | 禁 API 定稿宣称 |
| 3 | `AIParallelCoordinator` 升级更名 `AISubAgentManager`（同一变更集内改名 + 改调用点 + 改 e2e，**禁双轨并存**） | 禁新旧并存「多任务安全」宣称 |
| 4 | 只消费 `AITaskRun` / `AISession` / `AICoordinator`（宿主面）；**禁第二套调度器 / 锁 / 事件总线** | 禁「无平行机制」宣称 |
| 5 | 终态唯一在汇总门：`RunAggregatedGatesAsync` 全勾才可 `Completed`；Cancelled/Interrupted/Failed 计入未完结 → 红 | 禁子代理自报终态 |
| 6 | 演进路径 §9 每步（A1–A5）**须以其场景五面推演闭环为验收**（[scenario-drive-acceptance](scenario-drive-acceptance.md)），非测试全绿 | 禁以 e2e 冒充交付 |

写代码前另须过 [llm-gates](llm-gates.md) 与 043 开篇读前门闩。

## §1 目标与非目标

**目标**

| # | 目标 | 对应缺口 |
|---|------|---------|
| G1 | 子代理全生命周期可治理：Spawn / Interrupt / Resume / Cancel / Pause、超时熔断、死亡检测 | 3.1 无收束 · A.9 无撤单 |
| G2 | 失败不拖垮整体：监督策略（重启 / 降级 / 升级人）+ 失败/死亡**即时释放租约** | B11 失败隔离弱 |
| G3 | 运行中决策可同步：**旁路注入 / 打断回合双通道**，Revision 变更广播与定向重对齐 | 3.1 P3 纠偏广播缺失 |
| G4 | 动态派发与弹性：运行中加工作项、优先级调度、并行度可调 | 3.3 工作项无重跑处置 |
| G5 | 预算强制收束 + 成本核算 | `TotalBudget` 只算（[parallel-subagents §8 未落地面②](parallel-subagents.md)） |
| G6 | 结果汇总唯一权威（汇总门）增强：Cancelled/Interrupted/Failed 必答小结 + D5 跨子代理证据 | [parallel-subagents §8 未落地面④](parallel-subagents.md) |

一句话：**子代理管理 = 把「验证闭环」的并行执行从一次性波次派发，升级为可持续干预、可监督恢复、可预算收束的 reconcile 治理。验收仍回到单一汇总门。**

**非目标**

| # | 非目标 | 理由 |
|---|--------|------|
| N1 | 多线程并发驱动同一 `AIHost` | 单线程宿主模型（[parallel-subagents §5 B5](parallel-subagents.md)）；reconcile 循环是 async 交错，不是线程 |
| N2 | 真抢占 LLM 回合中途 | 打断粒度 = 回合边界 / provider 可取消点，诚实声明（N2 不冒充真抢占） |
| N3 | 自建消息总线 / 事件中心 | 决策轨迹唯一走 Agent 会话事件面（`AIDecisionEventKind` 扩展） |
| N4 | 第二套调度器 / 锁 | 只消费 `AITaskRun` / `AISession` / `AICoordinator` |
| N5 | 子代理自报终态 | 终态唯一在汇总门（B2） |

## §2 生命周期状态机

管理器持有精化状态 `AISubAgentState`（底层复用宿主 `AITaskRunStatus`，只映射、不重造）：

```text
Pending ──Spawn──► Spawned ──Start──► Running ──────────────────► Completed
   ▲                 │  ▲                │  ▲
   │                 │  │ Resume         │  │ Re-align（同 Revision 基线更新）
   │  监督重启        │  │                │  │
   └─────── Interrupted ◄────────── 决策同步（Revision+1 / 撤单 / 高优决策）
              │  │                      │
              │  └──► Cancelled（决策终止）│
              │                         │
              └──► Paused（显式暂停/快照）──► Running
                                              │
                     Failed ◄── 时长熔断/回合上限/租约后到拒绝/模型错误/预算超限
                     Dead   ◄── 心跳超时（死亡检测）→ 监督策略
```

**与 `AITaskRunStatus` 映射与迁移约束边表：**

| `AISubAgentState` | 底层 `AITaskRunStatus` | 语义与迁移约束 |
|---|---|---|
| `Pending` | Pending | 未派发 |
| `Spawned` | Pending（已建容器未 Start） | session + run 已建、回合未启动；**不再派发即预取整波租约**（租约惰性化，§5） |
| `Running` | Running | 回合执行中；`RunStepAsync` 逐回合推进 |
| `Paused` | Paused | 显式暂停（`AITaskRun.Checkpoint` 快照）；**同 Revision 才可 Resume** |
| `Interrupted` | Paused（打断点） | 决策同步打断：已检查点，**持有待决消息**；可重对齐回 Running、转 Cancelled、或按监督转 Pending 重启 |
| `Completed` | Completed | 有界回合完结 |
| `Failed` | Failed | 超时 / 租约冲突 / 模型错误 / 预算超限；必答小结 |
| `Cancelled` | Cancelled | 显式取消（撤单 / 决策终止） |
| `Dead` | Failed（标记 Dead） | 心跳超时检测；进入监督策略 |

**迁移约束（边表）：**

| 边 | 合法？ | 条件 |
|----|--------|------|
| Running → Paused | 合法 | 仅显式暂停 |
| Paused → Running | 合法 | 同 Revision 恢复 |
| Running → Interrupted | 合法 | 仅决策同步（Revision+1 / 撤单 / 高优决策） |
| Interrupted → Running | 合法 | 须完成「重对齐 + 租约重验」（Scope 可能变；冲突 → Failed） |
| Interrupted → Cancelled | 合法 | 决策终止 |
| Dead → 任意自行恢复 | **非法** | 只能经监督（重启 = Pending→Spawned→Running，或升级人） |
| 任意非 Completed 终态缺小结入账 | **非法** | `MarkDone` 前置校验：无小结不得入账（§7） |

> **现状底座**（只消费、不重造）：`AITaskRun` 已具 `MaxDurationMs` / `LastHeartbeatTicks` / `HeartbeatCount` / `IsStale(timeoutMs)`（时长熔断 + 心跳）+ Checkpoint/Resume 快照能力，manager 直接消费做死亡检测。

## §3 监督与恢复（失败隔离）

**失败分类与监督策略**（按工作项挂 `AISupervisionPolicy`）：

| 失败类 | 触发 | 监督策略 | 是否隔离 |
|---|---|---|---|
| `Transient` | 模型 provider 错误 / 网络 / LLM 超时 | 重启（≤ `MaxRestarts`，工作项回 Open 重派） | 是（其余在飞不受影响） |
| `LeaseConflict` | ToolPath 后到拒绝 | **禁自旋禁重试同一写面**：Failed + 小结 + 升级人（重新规划 Scope 或人裁决） | 是 |
| `BudgetExceeded` | 回合 / token / 时长超限 | 降级梯：wrap-up 注入 → 强制中断 → Failed + 升级人 | 是 |
| `Dead` | 心跳超时 | 重启（瞬态）或升级人（持久） | 是 |
| `PlanRejected` / `SpecContradiction` | 计划被拒 / Spec 矛盾 | 升级人（CCB），**禁自动重启** | 是 |
| 全局预算耗尽 | 聚合预算超限 | 全体降级 + 升级人 | 否（全局事件，但仍是「收束」非「崩溃」） |

**失败隔离保证：**

1. 每子代理独立 session + 独立租约（现状已满足）。
2. **失败/死亡即时释放租约**：现状 `ReleaseSession` 只在 `RunAllAsync` 末尾执行；改为 reconcile 循环在检测到 Failed/Dead 的**同一 tick 内** `ReleaseSession(run.Session.SessionId)`——被占路径立即归还，其余子代理可继续写（修 3.2 ② 的伴生问题）。
3. reconcile 循环继续派发剩余 Ready 工作项；失败项的依赖保持 Blocked（`AIRfcTaskGraph.Ready` 语义）。
4. 汇总门把任一 Failed/未完结判红（`Pending≠Passed`），杜绝「一个失败被淹没」。

## §4 消息与决策同步（双通道）

每子代理一个邮箱（Actor 模型消息面；**不发明事件总线**，投递由 manager reconcile 消费）：

| 通道 | 语义 | 实现 | 延迟 |
|---|---|---|---|
| **旁路注入**（`Interruptive=false`） | 不打断当前回合，下个回合边界生效 | 消息挂到 run 的 `PendingMessages`，reconcile 在下一次 `RunStepAsync` 前拼入 prompt（**增量 delta，不动前缀稳定块**，保护 KV cache 复用） | ≤1 回合 |
| **打断回合**（`Interruptive=true`） | 立即检查点当前回合，以决策为新 prompt 恢复 | `AITaskRun.Checkpoint()`（Running→Paused 快照）→ 置 Interrupted → 投递决策 → 重对齐后恢复 | 回合边界 / provider 可取消点（诚实声明，非真抢占） |

**决策广播 / 定向同步**（`PendingSyncDecisionAsync` 广播 / `SyncDecisionAsync` 定向）：

| 决策事件 | 目标 | 动作 |
|---|---|---|
| `revision-changed`（Revision+1） | 全部在飞（广播） | 各子代理：检查点 → Interrupted → 重建 `ContextBlock` 到新 Revision → **租约重验**（Scope 可能变；冲突 → Failed）→ 继续或 Cancelled |
| `work-item-rescope` | 定向工作项 | 重取 Scope 租约；后到拒绝 → Failed + 小结 |
| `cancel`（撤单） | Rfc 级（广播） | 取消未启动派发 + 在飞 → Cancelled + 释放该 Rfc 全部租约 + `airfc:cancelled` 事件 |
| `wrap-up`（预算压力） | 定向 / 广播 | 旁路注入「收束部分成果」，一个回合宽限后强制中断 |

**接线方式**：`DirectionLoop` 在 `/revise` / `/cancel` 后显式调 `manager.PendingSyncDecisionAsync(decision)`；决策事件 kind 走 `AIDecisionEventKind` 扩展（`subagent:interrupt` / `subagent:sync` / `subagent:cancel` / `subagent:rerun`），决策轨迹**单轨**可审计。

> **多事件混合仲裁（S-6）**：补充 + 纠正 + 新任务同时发生时，合并为**单次干预批次**（一次 Stop → 一次 Sync → 一次 Spawn），避免逐事件打断；每步写决策事件 kind；干预批次完成前受影响项保持「未完结」判定。

## §5 动态派发与弹性

- `AIRfcTaskGraph` 增强：`Add(workItem)`（构造校验复用：重复 Id / 未知依赖即时拒绝）、`Invalidate(workItemId)`（Acceptance 变更 → 已 Done 工作项失效重派，对应 3.3）、`Reprioritize(workItemId, priority)`。
- `AIRfcWorkItem` 增 `Priority`（int，高者先派）；`Ready()` 就绪面按**优先级 + 依赖序**排序。
- `SetParallelism(int max)`：**上调**立即多派；**下调**停止新派、在飞跑完（**禁为降并行度硬杀**）。
- `SpawnAsync` 派发前校验在飞数 < `MaxConcurrentSubAgents`（超限排下一波）。
- **租约惰性化（修 3.2 ②「同波预取假冲突」）**：把「派发即预取整波 Scope ToolPath 租约」改为「**首次真实写前取租约（惰性）**」——同一波内互不预占，先写者得、后写者 `Acquired=false`（后到拒绝语义不变，只是从「预取假冲突」变成「真实写冲突」）。

## §6 预算强制收束（TotalBudget 收束梯）

每子代理预算 `AISubAgentBudget`：回合上限（复用 `AISubAgentRun.maxSteps`）、token 上限（新，消费 `AISession.TotalUsage` / `UsageReported`）、时长熔断（`AITaskRun.MaxDurationMs`）。

**`TotalBudget` 强制收束落地**（[parallel-subagents §8 未落地面②](parallel-subagents.md)）：reconcile 每 tick 检查——

```text
if (Σ 各子代理 Usage + 汇总门开销) > TotalBudget(graph)  → 强制收束梯：
  ① 停止新派发
  ② 对在飞旁路注入 wrap-up（宽限 1 回合收束部分成果）
  ③ 宽限后强制中断（Interrupted → 收束小结）
  ④ 未收束完 → Failed(BudgetExceeded) + 小结 + 升级人（禁无界追加）
```

成本核算：每 run 的 `AITokenUsage` 聚合入 manager `TotalUsage`，随 `work_summary` / 新 `subagent:usage` 事件入决策轨迹（A.10 复盘底座）。

## §7 结果收集与汇总

- **必答小结（五字段）保持**；**Interrupted / Failed / Cancelled 也须交小结**（部分进度、卡在哪、绕过）——失败结果不静默。
- `AIRfcWorkItemStatus` 增 `Failed` / `Cancelled`（终态；依赖保持 Blocked；监督重启可把 Failed 转回 Open）。**能力面已具备**：`Failed` 入枚举 + wire 编解码；`AIRfcTaskGraph.MarkFailed`（必答小结、不进就绪面、不计 remaining、`SummaryOf` 可查）+ `AIParallelCoordinator.FinalizeRun` 按 `run.Status == Failed` 分派 `MarkFailed`（不折叠成 Done）；失败信号从瞬态 `AISubAgentRun.Status` 升级为工作项持久状态（跨会话可查）。
- 汇总门增强：`RunAggregatedGatesAsync`（现状：先验全部 `Completed` + `HasSummary` → 对合并后总工作区跑完整 D0–D7）**保持唯一权威**，补：① Cancelled/Interrupted/Partial 计入「未完结」→ 红（显式豁免须人确认，`Pending≠Passed` 语义不变）；② D5 跨子代理证据汇总（各 run 小结折叠为合并自审证据）；③ 失败处置沿用「红 → 升级人」。

## §8 API 草图

> 归属 `Arc.Agent.Harness`（基座主战场）；`Arc.Agent` 仅扩展 `AIDecisionEventKind`；`Arc.Agent.Harness.Coding` 零新增（汇总门判定已消费）。**核心决定：`AIParallelCoordinator` 升级更名 `AISubAgentManager`**（禁双轨，api-sketch §8 禁第二套 `*Coordinator`）。以下为**设计契约**；**A3 子集能力面已具备**：`AISubAgentMessage` / `AISubAgentDecision` / `EnqueueMessageAsync` / `PendingSyncDecisionAsync` / `SyncDecisionAsync` / `FindRun` / `AISubAgentRun.PendingMessages` 与 `AIDecisionEventKind.SubagentSync` 均实现（可执行用例 `arc_ai_decision_sync_e2e`），其余 API（`InterruptAsync` / `ResumeAsync` / `CancelAsync(runId)` / `SpawnAsync` / `DispatchDynamicAsync` / `ReconcileAsync` / `SetParallelism` / `AISupervisionPolicy` / `AISubAgentBudget` / graph `Add`/`Invalidate`/`Reprioritize`）仍为**设计态（诚实缺口），未实现**。

```as
// 归属：Arc.Agent.Harness（AIParallelCoordinator 升级更名为 AISubAgentManager）
namespace Arc.Agent.Harness;

/// <summary>子代理精化生命周期状态（与宿主 AITaskRunStatus 映射，见 §2）。</summary>
public enum AISubAgentState {
    Pending, Spawned, Running, Interrupted, Paused,
    Completed, Failed, Cancelled, Dead,
}

public class AISubAgentManager {
    public int MaxConcurrentSubAgents { get; set; }      // 并行度上限
    public void SetParallelism(int max);                 // 动态弹性（§5）
    public int InFlightCount { get; }
    public AITokenUsage TotalUsage { get; }              // 成本核算（§6）
    public bool TotalBudgetExceeded { get; }

    // 生命周期（§2）
    public Task<AISubAgentRun> SpawnAsync(AIRfcWorkItem item, CancellationToken ct);     // Pending→Spawned
    public Task<AITaskRunStatus> RunAsync(AIRfcWorkItem item, CancellationToken ct);     // Spawned→Running（动态新项）
    public Task<bool> InterruptAsync(string runId, string reason, CancellationToken ct); // Running→Interrupted
    public Task<bool> ResumeAsync(string runId, CancellationToken ct);                   // Interrupted→Running（重对齐）
    public Task<bool> CancelAsync(string runId, CancellationToken ct);                   // →Cancelled
    public Task CancelPendingAsync(string rfcId, CancellationToken ct);                  // 撤单：未启动 + 在飞 + 租约全收

    // 决策同步（§4）
    public Task PendingSyncDecisionAsync(AISubAgentDecision decision, CancellationToken ct);      // 广播
    public Task SyncDecisionAsync(string runId, AISubAgentDecision decision, CancellationToken ct); // 定向
    public Task EnqueueMessageAsync(string runId, AISubAgentMessage message, CancellationToken ct); // 旁路注入

    // 调度（§5 / §6）
    public Task DispatchDynamicAsync(AIRfcWorkItem item, CancellationToken ct);          // 运行中新增
    public Task ReconcileAsync(CancellationToken ct);                                    // 调度循环（K8s 控制器式）
    public Task<List<AISubAgentRun>> RunAllAsync(AIRfcTaskGraph graph, CancellationToken ct); // 兼容既有入口（内部 = Reconcile 循环）

    public AISubAgentRun? FindRun(string runId);
}

public class AISubAgentRun {                            // 现有类型增强（基座）
    public string WorkItemId { get; set; }
    public int RfcRevision { get; set; }
    public AIWorkSummary? Summary { get; }
    public int Priority { get; set; }                   // 新增
    public AISubAgentState State { get; }               // 新增（manager 维护的精化状态）
    public List<AISubAgentMessage> PendingMessages { get; } // 新增（邮箱）
    public void SetSummary(AIWorkSummary summary);      // 已有（null 拒绝）
}

public class AISubAgentMessage {                        // 新增（基座）
    public string Kind;       // "revision-changed" | "decision-sync" | "work-item-rescope" | "wrap-up"
    public int RfcRevision;
    public string Payload;
    public bool Interruptive; // true = 打断回合；false = 旁路注入
}

public class AISubAgentDecision {                       // 新增（基座，广播 / 定向统一载荷）
    public string Kind;       // revision-changed | work-item-rescope | cancel | wrap-up
    public string RfcId;
    public int RfcRevision;
    public List<string> TargetWorkItems;                // 空 = 全部
    public string Reason;
}

public class AISupervisionPolicy {                      // 新增（基座）
    public int MaxRestarts;
    public bool AllowRestart;
    public bool AllowDegrade;
    public bool EscalateToHuman;                        // 默认 true
}

public class AISubAgentBudget {                         // 新增（基座）
    public int MaxTurns;
    public int MaxTokens;
    public long MaxDurationMs;
}
```

## §9 演进路径（A1–A5）

> 每步「完成」= **其场景五面推演闭环**（[scenario-drive-acceptance](scenario-drive-acceptance.md)）——B 面（真实代码路径）落定 + 五面无断点；e2e 只是 B 面证据之一。依赖前置：S0（SR-1 `AIPlan.Id` + AIRfc/AIPlan/门状态持久化前置，见 [conflict-branch §10](conflict-branch.md)）。

| 步 | 内容 | 依赖 | 验收（场景五面推演闭环） |
|---|---|---|---|
| A1 | reconcile 循环化 `RunAllAsync`（turn-stepping）；租约惰性化（§5）；失败/死亡即时释放租约（§3） | S0 | **能力面**：`RunAllAsync` 为 reconcile 循环（每 tick 派发新 Ready → `RunStepAsync` 逐回合推进 → 心跳/时长熔断 → 终结同 tick 入账 + `ReleaseSession` 即时释放 → 预算守护）；租约惰性化经 `AISubAgentLeaseGate`（`Arc.Agent`）→ sandbox 调度层 `LeaseGate`，首次真实写前按路径 `Acquire(ToolPath)`。**3.2**：同波预取假冲突消除（`arc_ai_parallel_lazy_lease_overlap_e2e`）、首写真冲突仍后到拒绝（`arc_ai_parallel_toolpath_conflict_e2e`）、死亡/失败即时释放（`arc_ai_parallel_immediate_release_e2e`）。**诚实缺口**：`TotalBudget` 超限强制收束完整版仍设计态（A1 为步数预算守护最小版，归 A4） |
| A2 | 生命周期状态机（§2）+ Spawn / Interrupt / Resume / Cancel / CancelPending | A1 | **能力面**：`AISubAgentState`（Pending/Spawned/Running/Interrupted/Paused/Completed/Failed/Cancelled/Dead，与 `AITaskRunStatus` 映射）+ `AISubAgentRun.Spawn/Interrupt/Reap` + `AIParallelCoordinator.CancelPendingAsync`（取消未启动 + 中断在飞 + 联动 CTS + 即时释放租约 + 回收 Dead）+ `AIRfcTaskGraph.MarkCancelled` + `AIRfcWorkItemStatus.Cancelled` + `AIRfcStatus.Cancelled`（`CancelRfc`）+ `airfc:cancelled` / `subagent:interrupt` 事件 kind；**修取消被吞**（`RunAllAsync` 不再无条件 `Complete()` 覆盖，ct 取消/撤单 → Cancelled + 必答小结，汇总门按未完结红）。可执行用例 `arc_ai_cancel_pending_e2e`。**A.9**：撤单收束闭环。**诚实缺口**：`/cancel` REPL 接线 + keep-wip/rollback 处置选项 |
| A3 | 决策广播（§4：Revision 订阅 + `PendingSyncDecisionAsync` + 重对齐 + 租约重验） | A2 | **能力面**：`AISubAgentMessage`（Kind/Interruptive/Payload）+ `AISubAgentDecision`（Kind/RfcId/RfcRevision/TargetWorkItems/Reason）落 §8；`EnqueueMessageAsync` 旁路注入（soft：`PendingMessages` 邮箱，reconcile 回合前拼入 prompt delta，不动前缀稳定块）+ `PendingSyncDecisionAsync` 广播 / `SyncDecisionAsync` 定向（hard）——revision-changed → 在飞检查点（`CheckpointInterrupt` → Interrupted）→ 重建 ContextBlock 到新 Revision（`Realign`）→ **租约重验**（`AISubAgentLeaseGate.Revalidate`：Scope 变且新增写面被占 → 冲突 → Failed + 必答小结；不变 → 继续）；work-item-rescope → 定向重取 Scope 租约（后到拒绝 → Failed）；wrap-up → 旁路注入收束部分成果；决策事件 `subagent:sync`（`AIDecisionEventKind.SubagentSync`）入各 run 会话轨迹；**/revise 接线**：`AIHarnessSession.ReviseRfc` 成功后经 `RfcRevisionChanged` 钩子显式广播。单线程宿主（reconcile 循环内同步消费，非多线程）。可执行用例 `arc_ai_decision_sync_e2e`。**3.1**：运行中纠偏广播 → 在飞重对齐 / 冲突收束闭环 |
| A4 | 动态派发（§5：graph.AttachItem / SpawnAsync / SetParallelism）+ 预算强制收束（§6） | A3 | **能力面**：`AIRfcTaskGraph.AttachItem`（运行中增项 + 重复 Id / 未知依赖即时拒绝）+ `AIParallelCoordinator.SpawnAsync`（派发原语：派发前校验在飞数 < `MaxConcurrentSubAgents`，惰性取 Scope ToolPath 租约）+ `SetParallelism`（上调立即多派、下调停止新派在飞跑完，禁硬杀）；**TotalBudget 强制收束梯**（从「只算」到「强制」）：reconcile 每 tick 检查 Σ子代理回合用量 > `TotalBudget` → ① 停止新派发 ② wrap-up 旁路注入（宽限 1 回合）③ 宽限后强制中断 → 收束小结 ④ 未收束 → Failed(BudgetExceeded) + 小结 + 升级人。可执行用例 `arc_ai_dynamic_attach_spawn_e2e`（AttachItem 校验 / 运行中增项自动派发 / SpawnAsync 原语）+ `arc_ai_set_parallelism_e2e`（clamp + 上调 + 下调）+ `arc_ai_budget_enforce_e2e`（wrap-up → 宽限 → Failed(BudgetExceeded) + 小结 + subagent:usage 事件）。**3.3** 工作项重跑处置、**B11** 预算超限可复现闭环。**诚实缺口**：`AIRfcWorkItem.Priority` + `Reprioritize` / `Invalidate`（重跑处置，§5 其余项）仍设计态 |
| A5 | 成本核算 / 观测（§6/§7：TotalUsage + subagent:usage 事件） | A4 | **能力面**：`AIParallelCoordinator.TotalUsage`（聚合各 run 的 `AITokenUsage` token/轮次统计，随终结入账）+ `TotalTurns` + `TotalBudgetExceeded` + `InFlightCount`（成本观测面）+ `subagent:usage` 决策事件（`AIDecisionEventKind.SubagentUsage` + wire 编解码）入各 run 会话轨迹（单轨）；`DeepSeekResponseParser` 非流式路径回填 `reply.Usage`（对齐 OpenAI，修非流式无用量缺口，A5 前置）。可执行用例 `arc_ai_total_usage_e2e`（2 run × 1 回合 = total 32 / prompt 14 / completion 18 + TotalTurns=2 + subagent:usage 事件）。**B7** 成本可观测闭环。**诚实缺口**：汇总门增强（§7：D5 跨子代理证据折叠 + 未完结口径）仍设计态 |

## §10 与现有组件映射（升级更名，禁双轨）

| 现状（能力面） | 目标（方案 A） | 变化性质 |
|---|---|---|
| `AIParallelCoordinator`（波内 await 串行） | `AISubAgentManager`（reconcile 循环） | **结构性升级（改名 + 调度模型）**；同一变更集同步调用点与 e2e，禁双轨 |
| `DispatchAsync` 派发即预取整波租约 | `SpawnAsync` 惰性取租约（首次写前） | 修 3.2 ② 假冲突 |
| `RunAllAsync` await 到完结 | `RunAllAsync` = Reconcile 循环（turn-stepping） | 干预 / 动态派发 / 死亡检测的前提 |
| `AISubAgentRun`（WorkItemId / RfcRevision / Summary） | 增 `State` / `Priority` / `PendingMessages` | 增强 |
| `AIRfcTaskGraph`（Ready / MarkInProgress / MarkDone） | 增 `Add` / `Invalidate` / `Reprioritize` + `AIRfcWorkItemStatus.Failed/Cancelled` | 动态派发 / 重跑 |
| `AITaskRun`（心跳 / 熔断 / 快照） | 直接消费 + 死亡检测接 manager | 只消费 |
| `TotalBudget`（公式） | reconcile 强制收束梯（§6） | 落地 [parallel-subagents §8 未落地面②](parallel-subagents.md) |
| `RunAggregatedGatesAsync` | 不变（唯一权威）+ Cancelled/Interrupted 口径 + D5 跨子代理证据 | 增强 |
| `AISession`（MaxTurns / TotalUsage） | 消费为每子代理预算（`AISubAgentBudget`） | 接线 |

> **未落地面（诚实清单）**：A1（reconcile 循环 / 惰性租约门 / 即时释放）、A2（生命周期状态机 / Spawn/Interrupt/Reap/CancelPendingAsync / MarkCancelled / `AIRfcWorkItemStatus.Cancelled` / `AIRfcStatus.Cancelled` / 修取消被吞）、A3（`AISubAgentMessage` / `AISubAgentDecision` / 旁路注入 / 广播 / 定向 / revision-changed 重对齐 + 租约重验 / work-item-rescope / wrap-up / `subagent:sync` 事件 / `ReviseRfc` 广播钩子）、A4（`graph.AttachItem` 动态增项 / `SpawnAsync` 派发原语 / `SetParallelism` 弹性 / `TotalBudget` 强制收束梯）、A5（`TotalUsage` / `TotalTurns` / `TotalBudgetExceeded` / `InFlightCount` 成本观测面 + `subagent:usage` 决策事件 + DeepSeek 非流式 usage 回填）均已具备能力面，可执行用例见 §9。**剩余设计态（诚实缺口，未实现）**：`AISubAgentManager` 更名（现状保留 `AIParallelCoordinator` 类名，注释标注演进方向，禁双轨）；`ResumeAsync`/`CancelAsync(runId)`；`AISupervisionPolicy` / `AISubAgentBudget`；graph `Add`/`Invalidate`/`Reprioritize` + `AIRfcWorkItem.Priority`（重跑处置）；汇总门增强（§7：D5 跨子代理证据折叠 + 未完结口径）。未全部落地前不得宣称「子代理管理完成」。

---

[返回 043(../../043-harness.md) · [并行子代理（P3）](parallel-subagents.md) · [冲突分支（方案 B）](conflict-branch.md) · [真实场景运转协议](scenario-operation.md) · [references 索引](index.md)
