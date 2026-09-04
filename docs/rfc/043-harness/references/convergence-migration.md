# 收敛迁移（M0–M9）

> 关联 [043(../../043-harness.md) §11；目标 API 见 [api-sketch](api-sketch.md)；目标目录见 [package-layout](package-layout.md)。本子项给出从**首切片试探**（`HarnessAnchor` / `PlanSpec` / `HarnessEventLog` / 基座内 `quality.*`）到架构终态的有序迁移。每步独立验收；**未完成对应步禁止宣称该里程碑完成**。

## §0 宣称门闩

| 规则 | 契约 |
|------|------|
| 步进宣称 | 仅当该步「验收」列全部满足，方可标记该 `M*` 完成 |
| 禁止跳步宣称 | 不得因后续步局部动工而倒签前步完成 |
| 禁止假完成 | 见各步「假完成风险」；触碰任一条即**未完成** |
| 首切片地位 | `std/AI/Agent.Harness` 当前实现 = 试探；绿构建 ≠ 架构收敛完成 |
| 文档与代码 | 语义变更须同变更集；本 references 锁方向，不代替代码验收 |

## 迁移总览

```text
M0 文档锁
 → M1 AILeaseKey
 → M2 Fs 经 Coordinator
 → M3 Plan 租约
 → M4 AIRfc 删 PlanSpec
 → M5 RfcSpec 租约 + Agent 事件
 → M6 删 HarnessEventLog
 → M7 quality 迁 Coding
 → M8 DoD Pending≠Passed
 → M9 ArcAgent 薄组装
```

---

## M0 文档锁

| 面 | 内容 |
|----|------|
| **动作** | 锁定分层：Agent → Harness → Coding；Plan = `AIPlan` 引用；共用 `AICoordinator` 租约；quality/D0–D7 判定在 Coding；事件入 Agent 会话；确认首切片为试探。本步对应 api-sketch / package-layout / 本文落盘。 |
| **验收** | 043 与 references 无「PlanSpec 终态」「HarnessEventLog 永久双轨」「quality 焊死基座」表述；三篇 references 可互链；宣称纪律写明。 |
| **假完成风险** | 只改口号不改禁令表；文档仍把试探 API 写成稳定契约；未写「Pending≠Passed / 禁跳步宣称」。 |

## M1 AILeaseKey

| 面 | 内容 |
|----|------|
| **动作** | 在 `Arc.Agent` 将冲突织物升维：引入统一 `AILeaseKind { ToolPath, Plan, RfcSpec }` 与 `AILeaseKey`（或等价中性门面）；`AICoordinator` 能登记/拒绝/释放租约。 |
| **验收** | 三类 `AILeaseKind` 可编译可测；AIRfc / AIPlan / AITool **尚未**全接线也可，但公共类型已存在且无第二套锁类型。 |
| **假完成风险** | 仅注释「将要升维」无类型；或在 Harness 新建平行 `HarnessCoordinator`。 |

## M2 Fs 经 Coordinator

| 面 | 内容 |
|----|------|
| **动作** | 文件系统副作用路径统一走 `AILeaseKind.ToolPath`（现有写协调升维）；工具写盘须获租约再提交。 |
| **验收** | 两会话争用同一路径 → 后到者冲突可观测；合法写经 Acquire → Commit → Release；无绕过 Coordinator 的默认写盘捷径（受沙箱约束的工具面）。 |
| **假完成风险** | 测试只覆盖单会话；或「登记但不强制」导致仍可静默并行写。 |

## M3 Plan 租约

| 面 | 内容 |
|----|------|
| **动作** | `AIPlan` 修订 / 步进 / 门闩相关写路径消费 `AILeaseKind.Plan`；与 PlanGate 语义一致，不另发明计划锁。 |
| **验收** | 并行 Task 对同一 `PlanId` 冲突可测；Plan 状态机变更有租约审计点；Harness **未**再包第二计划状态机。 |
| **假完成风险** | 只在文档写「Plan 共用租约」，计划 API 仍无租约参数/门面调用；或 Harness 内残留 `PlanSpec` 写路径当正式 API。 |

## M4 AIRfc 删 PlanSpec

| 面 | 内容 |
|----|------|
| **动作** | 落地 `AIRfc` 聚合根与 `AIRfcRuntime`（Create / Revise / AttachPlan / BindWorkItem）；Plan 面 = `AIPlan` 引用（`PlanId` + 可选句柄）；**删除** `PlanSpec` / `HarnessAnchor` 作为公开事实源。 |
| **验收** | 代码与文档均无 `PlanSpec` 终态 API；`AttachPlan` 只引用 `AIPlan`；纠偏升版走 `AIRfc.Revision`；工作项可绑定 Session/TaskRun。 |
| **假完成风险** | 改名 `HarnessAnchor`→`AIRfc` 但仍内嵌平行计划字段；或 `PlanSpec` 改名残留；或「双写」AIRfc + Anchor。 |

## M5 RfcSpec 租约 + Agent 事件

| 面 | 内容 |
|----|------|
| **动作** | `AIRfcRuntime` 写路径持 `AILeaseKind.RfcSpec`；`airfc:created` / `airfc:revised` / `airfc:rejected` / `checkpoint:*` / `work_summary` 写入 **Agent 会话事件**（与 038 approval 并列）。 |
| **验收** | 两任务同时升版同一 `RfcId` → **后到拒绝**且可审计（无排队）；会话可重放关键轨迹事件；Harness **不再**作为轨迹唯一存储。 |
| **假完成风险** | 事件仍只进 `HarnessEventLog`、会话事件空挂；或 RfcSpec 租约可绕过（Runtime 直写字段）。 |

## M6 删 HarnessEventLog

| 面 | 内容 |
|----|------|
| **动作** | 删除 `HarnessEventLog` / 永久双轨 `HarnessEvent` 公共 API；调用方改读 Agent 会话事件；清理 `AIHarnessSession` 对独立日志的持有。 |
| **验收** | 仓库无 `HarnessEventLog` 类型；轨迹测试只断言会话事件；绿点/回滚/小结均可在会话事件中复盘。 |
| **假完成风险** | 「标记 obsolete 但保留默认路径」；或 example 私藏一份平行日志当真相源。 |

## M7 quality 迁 Coding

| 面 | 内容 |
|----|------|
| **动作** | 新建 `std/AI/Agent.Harness.Coding`（见 [package-layout](package-layout.md)）；迁入 `QualityTools` / `QualityCli`；实现 `CodingDoDGateEvaluator`；基座 `AIDoDOrchestrator` 改为委托 `IAIDoDGateEvaluator`，去掉写死 arc CLI。 |
| **验收** | 基座包不再包含 `Quality/`；Coding 包可独立引用；`quality.*` 从 Coding 注册；基座 `arc.toml` 不依赖 Coding。 |
| **假完成风险** | 仅复制文件到 Coding、基座仍保留一份；或 namespace 未改仍落在 `Arc.Agent.Harness`；或 Orchestrator 仍直接调用 `QualityCli`。 |

## M8 DoD Pending≠Passed

| 面 | 内容 |
|----|------|
| **动作** | 硬化 DoD 语义：未接线 / 未跑门 = `Pending`；`AllPassed` / `Completed` **不得**把 `Pending` 当通过；D1/D2/D4/D6 未接线须诚实 Pending（或明确 NeedsHuman），禁止假绿。 |
| **验收** | 单测：结果列表含 Pending → `AllPassed == false`；产品路径无法在 Pending 门存在时宣称计划 `Completed`；与 [definition-of-done](definition-of-done.md) 一致。 |
| **假完成风险** | UI 把 Pending 显示成「跳过=通过」；或自动门子集跑完即标 Completed，忽略未跑门。 |

## M9 ArcAgent 薄组装

| 面 | 内容 |
|----|------|
| **动作** | `examples/ArcAgent` 依赖 `Arc.Agent` + `Arc.Agent.Harness` + `Arc.Agent.Harness.Coding`（+ Provider）；组合根只组装：Host、AIRfcRuntime、Coding 评估器、quality 工具注册；无本地 PM/DoD/事件库实现。 |
| **验收** | `arc.toml` 依赖边完整；Program/Host 无 `PlanSpec`/`HarnessEventLog`/`QualityCli` 内联翻版；构建与冒烟路径证明三包接线。 |
| **假完成风险** | 依赖了 Coding 但未注册评估器，运行时仍走基座旧路径；或 example 内重新实现门判定「图省事」。 |

---

## 与规划里程碑的对照（非宣称替代）

| 本文 | 与 `docs/plan.md` Harness 行的大致对应 |
|------|----------------------------------------|
| M0 | H-0 文档锁延伸（references 补齐） |
| M1–M3 / M5 租约面 | H-2d（`AICoordinator` 升维） |
| M4 | H-2c（AIRfc + 删 PlanSpec） |
| M6 | H-2c（轨迹并入 Agent 事件） |
| M7 | H-2b（Coding 包 + quality 迁出） |
| M8 | H-2c/H-3 前置语义门闩（禁假绿） |
| M9 | H-2 终端薄壳在 Coding 拆分后的再验收 |

> 上表仅便检索；**完成以本文各步验收列为准**，不以规划表勾选单独宣称。

---

[返回 043(../../043-harness.md) · [API 草图](api-sketch.md) · [包布局](package-layout.md) · [references 索引](index.md)
