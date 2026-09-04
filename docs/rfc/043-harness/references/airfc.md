# AIRfc 体系

> 关联 [043 Coding Agent Harness 工程(../../043-harness.md)（读前门闩 · 宣称纪律 · §2 · §10 · §11）。本子项定义 Harness 基座内的**小型项目管理 / 需求本尊运行时**（`AIRfc`）。纠偏分流细节见 [纠偏协议](anchor-correction-protocol.md)。

## 0. 宣称门闩

未满足下列全部项前，**禁止**宣称 AIRfc「完成 / Completed / 终态 / 已收敛」：

| # | 门闩 | 未过时 |
|---|------|--------|
| 1 | 聚合根字段级契约与本文 §3 一致；Plan = `AIPlan` 引用 | 禁对外 API 定稿宣称 |
| 2 | 无平行 `PlanSpec`；无永久 `HarnessEventLog` | 禁「已对齐 043」宣称 |
| 3 | 双 Revision 规则（§4）有可测边表；合法/非法边有测试或验收用例 | 禁「版本协议完成」宣称 |
| 4 | 冲突织物只消费宿主面，不平行实现锁 | 禁「多任务安全完成」宣称 |
| 5 | 决策轨迹写入 Agent 会话事件；可审计 | 禁「轨迹完备」宣称 |
| 6 | Coding 领域若宣称计划完成，须 D0–D7 全勾（见 [definition-of-done](definition-of-done.md)） | 禁 `AIPlan.Completed` |

写代码前另须过 [llm-gates](llm-gates.md) 与 043 开篇读前门闩。

## 1. 目标

| # | 目标 |
|---|------|
| G1 | **AIRfc** 成为方向环与执行环的唯一事实源（跨任务、跨版本） |
| G2 | Spec 面（Intention / Design / Acceptance）聚合在单一根内；Plan 面直接引用 **`AIPlan`** |
| G3 | 纠偏 = 增量升版；修复 = 不升 Spec；轨迹可审计 |
| G4 | 与 `AIPlan` / `AITool` **共用**冲突织物（见 [conflict-fabric](conflict-fabric.md)） |
| G5 | 决策轨迹落入 Agent 会话事件，不另起永久日志库 |

一句话：**AIRfc = 跨任务、跨版本的需求与交付状态唯一事实源。**

## 2. 非目标

| # | 非目标 |
|---|--------|
| N1 | 企业级 PLM / Jira / 完整项目管理套件 |
| N2 | 平行 `PlanSpec` 或第二套计划状态机 |
| N3 | 永久独立的 `HarnessEventLog`（双轨事件库） |
| N4 | 把试探类型 `HarnessAnchor` 等固化为产品终态 API |
| N5 | 在 Harness 内再造 HITL / 冲突锁 / Tool 沙箱 |
| N6 | 与仓库 `docs/rfc/` 文档格式混同（`AIRfc` 是运行时工件） |

## 3. 类型契约（聚合根 · 字段级）

AIRfc 是**单一聚合根**；下列为内部必填面，禁止拆成四个独立管理系统。

| 字段 / 面 | 类型契约 | 约束 |
|-----------|----------|------|
| `Id` | 稳定标识 | 跨会话不变 |
| `Revision` | 单调非负整数 | 仅 Spec 语义变更时 +1（见 §4） |
| `Intention` | 可感知结果描述 | 非技术实现细节 |
| `Design` | 远见 / 收敛 / 结构 / 模式 / 决策理由 | 变更须评审语义 |
| `Acceptance` | 场景 + 断言集合 | 测试先行锁定；随升版同步 |
| `Plan` | **`AIPlan` 引用**（Id / 句柄） | **禁止**内嵌平行 `PlanSpec` |
| `WorkItems[]` | 工作项集合 | 可跨会话并行；可关联 `AITaskRun` |
| `Status` | **`AIRfcStatus`** 运行态枚举（Active / Superseded / Rejected / Contested / Frozen / Closed / Cancelled） | 非法迁移见 §4 |

```text
AIRfc (聚合根, Revision N)
  ├── Intention / Design / Acceptance
  ├── Plan ──────────────► AIPlan + PlanGate（038，直接复用）
  ├── WorkItems[] ───────► 可跨会话并行；可关联 AITaskRun
  └── Trail ─────────────► 决策事件写入 Agent 会话事件（禁永久 HarnessEventLog）
```

## 4. 状态机或协议（Revision 合法 / 非法边）

### 4.1 双 Revision 规则

| 变更种类 | 只 `AIPlan.Revision++` | 只 `AIRfc.Revision++` | 两者都可能 | 说明 |
|----------|------------------------|------------------------|------------|------|
| 计划步骤增删改、验证点调整、门闩步进 | ✓ | — | — | Spec 意图未变；Plan 面走 038 协议 |
| Intention / Design / Acceptance 语义变更 | — | ✓ | — | 纠偏升版；执行环重对齐 |
| 用户拒绝设计/验收后改方向 | — | ✓ | — | 写 `airfc:revised` / `airfc:rejected` |
| Plan 引用换绑到另一 `AIPlan` 且代表新交付路径 | — | ✓ | 可选同步 Plan 侧 | 视为 Spec 计划面语义变更 |
| 验证失败后的实现修复（代码迭代） | — | — | — | **修复**：两边 Revision **都不**因修复本身 +1 |
| 仅状态展示/缓存刷新 | — | — | — | 非法升版源 |

### 4.2 AIRfc Revision 边表

| 边 | 合法？ | 条件 |
|----|--------|------|
| Active → Active（Revision N → N+1） | 合法 | Spec 必填面增量升版；写会话事件轨迹 |
| Active → Superseded | 合法 | 被更新 Revision 取代；旧版只读审计 |
| Active → Rejected | 合法 | 人拒绝；轨迹可审计 |
| Rejected → Active（同 Revision） | **非法** | 须新 Revision 再入 Active |
| Superseded → Active | **非法（回滚例外）** | 不可复活旧版为当前事实源；**唯一例外**：绿点回滚（场景 3.4）经 `RestoreRevision` 把 Superseded 目标版转回 Active，**必须持 RfcSpec 租约**（后到拒绝） |
| Revision 回退（N → N−1） | **非法（回滚例外）** | 只前进；审计靠事件；**唯一例外**：绿点回滚（场景 3.4）恢复旧版本号（不递增），**必须持 RfcSpec 租约**（后到拒绝） |
| Spec 变更却不 +1 | **非法** | 执行环只认最新版 |
| 修复迭代触发 Spec Revision+1 | **非法** | 修复不升 Spec |
| Active → Contested | 合法 | 多来源需求冲突（A.1，子代理管理方案 A）；冲突期间禁修订/拒绝 |
| Contested → Active | 合法 | 冲突解决（`ResolveContested`）；仅 Contested 生效 |
| Active → Frozen | 合法 | 冻结窗口开始（A.2）；冻结期间禁 Revise/Reject |
| Frozen → Active | 合法 | 解冻（`UnfreezeRfc`）；仅 Frozen 生效 |
| Active/Frozen → Closed | 合法 | D7 通过收口终态；禁再 Revise/Reject |
| Active/Frozen/Rejected → Cancelled | 合法 | 撤单终态（A.9）；只读 |
| Closed → 任意 | **非法** | 终态只读 |
| Cancelled → 任意 | **非法** | 终态只读 |
| Frozen/Contested/Closed/Cancelled → Revise | **非法** | 冻结/冲突/收口/撤单均禁修订（仅 Active/Rejected 可经 `RequireMutable` 修订） |

> 新态运行时入口：`AIRfcRuntime.MarkContested / ResolveContested / ResolveContestedWithSpec /
> RejectContested / FreezeRfc / UnfreezeRfc / CloseRfc / CancelRfc`；会话包装
> `AIHarnessSession.ContendRfc / ResolveRfc / FreezeRfc / UnfreezeRfc / CloseRfc / CancelRfc`
> （Close/Cancel 记 `airfc:closed` / `airfc:cancelled` 决策事件）。B1 冲突裁决入口：
> `AIConflictResolver.ResolveAsync / RejectAsync` + `AIHarnessSession.ResolveConflictAsync /
> RejectConflictAsync`（记 `conflict:*` / `airfc:resolved` 决策事件；resolvedBy 必须显式人）。
> 工作项新终态：`AIRfcWorkItemStatus.Failed / Cancelled`（不参与就绪面；依赖保持 `Blocked`；
> 不计入 remaining）。`Failed` 由 `AIRfcTaskGraph.MarkFailed` 承载、`AIParallelCoordinator.FinalizeRun`
> 按 `run.Status == Failed` 分派——失败信号从瞬态 `AISubAgentRun.Status` 升级为工作项持久状态
> （跨会话可查），`AIDoDOrchestrator.RunAggregatedGatesAsync` 先验 Failed/Cancelled 即红。
>
> > **回滚例外（单一权威）**：`Superseded → Active` 与 `Revision 回退` 两条非法边**仅**在绿点回滚
> > （场景 3.4 `AIRfcRuntime.RestoreRevision`，由 `CheckpointRollbackAsync` 联动调用）时为例外放行，
> > 且**必须持 `AILeaseKind.RfcSpec` 租约**（与 Create/Revise 等写路径一致，后到拒绝）——回滚是
> > Spec 面写，**禁止绕过租约**；未持租约 → 返回 null、不落变更。

### 4.3 与 AIPlan 的衔接

- PlanGate / `AIPlan` 状态机以 [038](../../038-ai-host.md) 为准；
- `AIPlan.Completed` **不得**仅因计划步骤跑完而宣称——Coding 领域须 D0–D7 全勾，并遵守 043 [宣称纪律(../../043-harness.md)。

### 4.4 L2 Spec 矛盾检测与 CCB 裁决（方案 B B1）

- **机器检测（可判则判）**：`AIHarnessSession.ReviseRfc` 升版前经 `AISpecConflictDetector` 做字段级结构化 diff（`AIAcceptanceSpec.Items` 条目级比对）——**异来源**（`AIRfc.Source` ≠ 发起来源）覆盖同 acceptance 项（同索引内容变化）→ 反方向覆盖信号 → **不落新 Revision**，当前版标 `AIRfcStatus.Contested` + 登记 `AIConflictRecord`（`AIConflictKind.SpecContradiction`；Resources/Parties[来源A,来源B]/Evidence=diff；Status=Open）+ `conflict:detected` 决策事件。同来源修订 = 多轮讨论 refine（场景 1.3），不判矛盾。
- **人 CCB 裁决（唯一入口，禁自动选胜者）**：`AIConflictResolver.ResolveAsync(conflictId, decision, reason, resolvedBy)`——`resolvedBy` 必须显式人（空 → false）；`decision` 前缀 `accept-after` 采纳被拦截方向，否则维持冲突前方向。裁决后 `AIRfcRuntime.ResolveContestedWithSpec` 写 **Active 新 Revision 基线** + `conflict:resolved` / `airfc:resolved` 决策事件。`RejectAsync` → 记录 Rejected + AIRfc Contested → Rejected（方向被否，可新 Revision 再入 Active）+ `conflict:rejected`。
- **`/conflict` REPL**：列出 Open 冲突（方向/来源/evidence）+ resolve/reject 人 CCB 交互（`--by=<CCB>` 显式裁决人）。
- 冲突面为 `AIAcceptanceSpec.Items` 结构化条目；纯文本 Scenarios/Assertions 面无机器可比结构，不触发 L2（A.1 口径）。
- 机制细节与演进见 [conflict-branch](conflict-branch.md)（方案 B §2/§5/§9 B1）。

## 5. 禁令

| # | 禁令 |
|---|------|
| B1 | 禁止平行 `PlanSpec` / 第二计划状态机 |
| B2 | 禁止永久 `HarnessEventLog`；轨迹只写入 Agent 会话事件 |
| B3 | 禁止 AIRfc / AIPlan / AITool 各搞各的冲突锁 |
| B4 | 禁止把 `HarnessAnchor` 等试探类型当终态抄写或对外定稿 |
| B5 | 禁止未过 §0 / [llm-gates](llm-gates.md) 即宣称完成 |
| B6 | 禁止在对话散落「口头 Spec」替代当前 AIRfc Revision |
| B7 | 禁止 Harness 再包一层同名 HITL / 事件 / Plan API（应回修 Agent） |

## 6. 验收 DoD

| # | 验收项 | 通过信号 |
|---|--------|----------|
| D1 | 字段级契约可对照 §3 实现或草图 | [api-sketch](api-sketch.md) 对齐；无 `PlanSpec` |
| D2 | 双 Revision 边表有正反用例 | 合法边可走；非法边被拒 |
| D3 | 纠偏升版写会话事件 | 可见 `airfc:revised` 等，无独立永久日志库 |
| D4 | 冲突织物只消费 | 见 [conflict-fabric](conflict-fabric.md)；无平行锁 |
| D5 | 分层归属正确 | AIRfc 在 `Arc.Agent.Harness`；Coding 不拥有第二套 |
| D6 | 宣称门闩可勾 | §0 全绿后方可书面宣布 |
| D7 | 新态迁移与持久化有验收用例 | `arc_ai_rfc_status_e2e`（Contested/Frozen/Closed/Cancelled 正反用例 + 既有语义不回归）；`arc_ai_rfc_persistence_e2e`（SaveRfc → 新进程 RestoreRfc → Revision/Status/WorkItems/PlanId 重建）；`arc_ai_conflict_resolve_e2e`（B1 L2 检测 + 人 CCB 裁决 + 新基线 + Reject） |

## 7. 上下游链接

| 方向 | 链接 | 关系 |
|------|------|------|
| 上游 Plan / HITL / 事件 | [038 AI 宿主](../../038-ai-host.md) | `AIPlan`、PlanGate、会话事件、协调面 |
| 冲突织物 | [conflict-fabric](conflict-fabric.md) · [038](../../038-ai-host.md) | 共用租约；本篇只摘要 |
| 写代码门闩 | [llm-gates](llm-gates.md) | LLM 动手前硬约束 |
| 纠偏分流 | [anchor-correction-protocol](anchor-correction-protocol.md) | 修复 vs 升版 |
| 主架构 | [043(../../043-harness.md) | 分层、宣称纪律、试探≠终态 |
| 包与迁移 | [package-layout](package-layout.md) · [convergence-migration](convergence-migration.md) | 落地与收敛 |

### 冲突织物摘要

硬约束：**AIRfc · AIPlan · AITool 共用同一套**跨会话、多任务并行冲突处理（租约 / 授予 / 冲突检测 / 原子提交）。AIRfc 工作项或 Spec 写、AIPlan 修订、AITool 副作用路径均走同一织物；禁止三套锁。演进落点在 038 协调面升维；细节见 [conflict-fabric](conflict-fabric.md)。

### 决策轨迹

`airfc:created` / `airfc:revised` / `airfc:rejected` / `airfc:closed` / `airfc:cancelled` / `airfc:resolved`（B1 冲突裁决新基线）/ `checkpoint:*` / `conflict:detected` / `conflict:resolved` / `conflict:rejected`（B1 冲突全程）与 038 `approval` 并列（并行子代理撤单收束另记 `subagent:interrupt` / 决策同步记 `subagent:sync`，见 [subagent-management](subagent-management.md)），**一律写入 Agent 会话事件**；**禁止**永久 `HarnessEventLog` 双轨。

## 8. 历史别名

文档与首切片代码中的「锚点 / `HarnessAnchor`」以及旧称「锚点四件套」均指 **AIRfc Spec 聚合** 的过渡称呼；**正道产品名一律 `AIRfc`**，不得再作对外产品名使用。试探实现收敛见 043 §11 与 [convergence-migration](convergence-migration.md)。

---

[返回 043(../../043-harness.md) · [纠偏协议](anchor-correction-protocol.md) · [llm-gates](llm-gates.md) · [references 索引](index.md)
