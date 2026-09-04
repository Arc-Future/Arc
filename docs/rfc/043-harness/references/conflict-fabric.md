# 冲突织物（AIRfc · AIPlan · AITool）

> 关联 [043](../../043-harness.md) · [AIRfc](airfc.md) · 宿主契约 [038 §13](../../038-ai-host.md)。  
> 本子项写 **消费约定与验收**；租约 API 草图与宿主面决策以 038 为准，此处不重复长文。  
> **试探 ≠ 终态**：`CommitPlan` 未覆盖 Plan 面——不得据此宣称「冲突织物已完成」。

## 0. 宣称门闩

未勾下列项，**禁止**宣称「冲突织物已落地 / 共用租约已完成」：

- [ ] 三 Kind（`ToolPath` / `Plan` / `RfcSpec`）走**同一** `AICoordinator` 登记表
- [ ] 冲突策略仅为**后到拒绝 + 可审计**（无排队、无自旋等待冒充协调）
- [ ] `AIRfc` / `AIPlanGate` / `[AITool]` 副作用路径**无**第二套锁
- [ ] PlanGate（未批准）与租约（谁可改）正交叠加，禁互替
- [ ] 决策轨迹经 Agent 会话事件追加面（如 `AppendDecisionEvent`），无永久双轨日志库

## 1. 目标

| 面 | 契约 |
|----|------|
| 一织物 | 跨会话、多任务并行时，AIRfc Spec 写、AIPlan 修订/步进、AITool 路径副作用共用一套租约语义 |
| 可审计 | 冲突与覆写可追溯（谁持有、谁被拒、路径覆写 hash） |
| 可组合 | Harness / Coding **只调用**宿主 Coordinator；缺能力则回修 Agent，不包装第二门面 |

## 2. 非目标

| 拒绝 | 说明 |
|------|------|
| 分布式锁 / 跨进程租约 | 本织物绑定单进程 `AIHost` 单线程宿主模型 |
| 排队等待先到者 | 后到一律拒绝；调用方自行退避或升级人 |
| 工作项单独成 Kind | 工作项争资源时争的是 `RfcId` / `PlanId` / `Path`，不另开锁表 |
| 用文件锁代替 Plan/RfcSpec 租约 | 三 Kind 键空间必须齐，禁「只锁文件」冒充完成 |

## 3. 类型契约（消费侧）

宿主升维面（权威草图见 [038 §13](../../038-ai-host.md)）：

| 符号 | 消费约定 |
|------|----------|
| `AILeaseKind` | `{ ToolPath, Plan, RfcSpec }` |
| `AILeaseKey` | `Kind + Norm(Id)`；`ToolPath` 经 `AIWorkspace.ResolvePath`；`Plan`=`plan:`+PlanId；`RfcSpec`=`airfc:`+RfcId |
| `AIResourceGrant` | `Acquired` / `HolderId`(=SessionId) / 可选 `TaskRunId`（审计元数据，非第二锁） |
| `Acquire` | 冲突 → `Acquired=false`；先到者不受阻 |
| `Release` / `ReleaseSession` | 显式放锁；Commit **不**自动放锁 |
| `Commit*` | `CommitAsync`(路径) / `CommitPlan` / `CommitRfcSpec`：仅 `Acquired` 时执行突变 |

**谁在何时 Acquire：**

| 消费方 | Kind | 临界区 |
|--------|------|--------|
| `AIToolSandbox` / Fs 写工具 | `ToolPath` | capability + PlanGate 通过后、落盘前 |
| `AIPlanGate`（`RevisePlan` / `MarkStepDone` 等突变） | `Plan` | 改 provider/计划状态前 |
| `AIRfcRuntime.Revise` / Spec 面写 | `RfcSpec` | Revision+1 与 Spec 突变前 |

## 4. 协议与正交关系

```text
副作用路径：
  PlanGate.Blocks? ──拦──► 对模型可见错误（未批准）
       │放行
       ▼
  Coordinator.Acquire(kind) ──拒──► 冲突结果（可审计；后到拒绝）
       │Acquired
       ▼
  Commit* → （稍后）Release / ReleaseSession
```

| 机制 | 管什么 | 不管什么 |
|------|--------|----------|
| PlanGate + `PlanGatedCapabilities` | 未批准计划能否执行受约束写能力 | 谁持有该 Plan/路径/Rfc |
| 冲突织物 | 跨会话谁可改同一资源 | 计划是否已批准 |
| HITL / 审批事件 | 人类放行语义 | 租约表本身 |

**决策事件 kind（写入 Agent 追加面，目标 API 名见 038）：** `airfc:created` / `airfc:revised` / `airfc:rejected` / `approval` / `checkpoint:*`。

## 5. 禁令

1. **禁止**为 AIRfc / AIPlan / AITool 分别实现冲突表或同名包装锁。
2. **禁止**冲突策略写成「拒绝或排队」二选一——正道锁定为**后到拒绝 + 可审计**。
3. **禁止**用 PlanGate 替代租约，或用租约替代 PlanGate。
4. **禁止**永久 `HarnessEventLog`（或改名）与 Agent 会话事件双轨并存。
5. **禁止** Commit 后静默放锁（破坏编辑间隙保护；与现路径写一致）。

## 6. 验收 DoD（三 Kind 冲突用例）

| # | 用例 | 期望 |
|---|------|------|
| C1 | 两 Session 对同一工作区路径 `Acquire(ToolPath)` | 先到 `Acquired=true`；后到 `false`；审计可查持有者 |
| C2 | 两 Task 同时对同一 `PlanId` `RevisePlan`/`MarkStepDone` | 仅持 `AILeaseKind.Plan` 者成功；另一方冲突可观测 |
| C3 | 两任务同时对同一 `RfcId` 升版 Spec | 仅持 `AILeaseKind.RfcSpec` 者 `Revision+1`；另一方拒绝；`airfc:revised` 仅成功路径追加 |

三者必须命中**同一** `AIHost.Coordinator` 实例登记表。缺任一 Kind 键空间 → 本门闩未通过。

## 7. 上下游链接

| 方向 | 文档 |
|------|------|
| 上游宿主 API | [038 §12 Plan](../../038-ai-host.md) · [038 §13 冲突织物](../../038-ai-host.md) |
| AIRfc 聚合与复用纪律 | [airfc](airfc.md) |
| Completed / D0–D7 | [definition-of-done](definition-of-done.md) |
| 开工/合入检查 | [llm-gates](llm-gates.md)（并行文档包） |
| 类型草图 | [api-sketch](api-sketch.md)（并行文档包） |
| 主文档 | [043](../../043-harness.md) |

## 8. 目标能力面（含诚实缺口标注）

| 面 | 目标能力面 |
|----|-----------|
| Coordinator | 三 Kind 通用 `Acquire` + `Commit*`（RfcSpec 已按消费侧接线；Plan 面 `CommitPlan` 尚待覆盖——诚实缺口） |
| Plan 突变 | `AILeaseKind.Plan` 包住 Gate 突变 |
| AIRfc | `RfcSpec` 租约 + Agent 事件轨迹 |
| 事件 | 唯一 `AppendDecisionEvent`（或等价） |

---

[返回 043](../../043-harness.md) · [AIRfc](airfc.md) · [038](../../038-ai-host.md) · [references 索引](index.md)
