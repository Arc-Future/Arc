# AIPlan 树形状态树（plan-tree）

> 关联 [043 Coding Agent Harness 工程(../../043-harness.md)（§2 · 宣称纪律 · §10 分层）· [AIRfc 体系](airfc.md) · [并行子代理（P3）](parallel-subagents.md) · [子代理管理（方案 A）](subagent-management.md) · [可执行 DoD](definition-of-done.md)。本子项定义 **AIPlan 树形状态树** 设计——把 `AIPlan` 从扁平 `List<AITaskStep>` 升级为树形状态树（`AIPlanNode` + 聚合状态机）。
>
> **能力面**：P1（单层树 + 聚合）已具备能力面；本子项为「去过度设计」收敛后的**最小必要版**（P1a/P1b/P1c 演进，见 §5）。

## §0 宣称门闩

未满足下列全部项前，**禁止**宣称「AIPlan 已树化 / 树形状态树完成」：

| # | 门闩 | 未过时 |
|---|------|--------|
| 1 | **P1 已具备能力面**——`AIPlanNode` / `AIPlanTree` / `AIPlanNodeStatus`（六态）已具备（单层树 + fail-closed 聚合 + FromFlat 迁移）；P1a/P1b/P1c 未具备能力面前 | 禁「AIPlan 多级树化完成」宣称 |
| 2 | 命名一律 `AI` 前缀；`AIPlanNode` 归属 `Arc.Agent`，只存 `RunId` **字符串引用**，不 import Harness 类型（依赖单向，043 §10） | 禁 API 定稿宣称 |
| 3 | **禁双轨**：迁移同变更集改名 + 改调用点，不引入 deprecated shim，不保留 `AITaskStep`/三套平行结构并存 | 禁新旧并存「零平行」宣称 |
| 4 | 演进 P1a–P1c 每步以其**场景五面推演闭环**验收（[scenario-drive-acceptance](scenario-drive-acceptance.md)），非测试全绿 | 禁 e2e 冒充交付 |
| 5 | 对齐 043 宣称纪律：根 `Completed` ⇔ 汇总门 D0–D7 全勾；叶不自报终态（[parallel-subagents](parallel-subagents.md) B2） | 禁叶自报终态 |

## §1 为什么之前「九态」过度（裁剪说明，防回潮）

真实项目推演审查确认：旧设计约一半抽象是多余的。裁剪映射如下，**已收敛、勿回潮**：

| 砍掉 | 理由 | 替代 |
|------|------|------|
| `AIPlanNodeKind.Checkpoint` | 根 = `AIPlanTree.Root`（`ParentId` 空）即可识别，不为唯一实例造枚举值 | `Kind` 只留 `Leaf`/`Group` |
| 九态 `Blocked` | 现状只是 `Ready()` 的瞬态派生（依赖未满足），非持久态 | 依赖未满 = `Pending` |
| 九态组级 `Verifying` | 汇总门锁根，组级 `Verifying` 是死状态 | 根专属 `RootVerifying` 标志（不进通用枚举） |
| 九态 `Skipped` | 后置；P1 用「改 `Completed` + 小结『跳过』」承载 | 无独立态 |
| `OrderAfter` | 保留 `DependsOn`，同一数据结构换名零收益；「树 + OrderAfter」≈复杂化 DAG | 字段名 = `DependsOn` |
| `OwnerSessionId` | 无多级委托路径，owner 恒 = 主代理，冗余 | 删 |
| `DelegatedSessionId` | 与 `RunId` 1:1 冗余 | 只留 `RunId` |
| `WorkItemId` | = `Id` 的迁移别名，违背禁双轨 | 删 |
| `EffectiveScope` | 派生无消费方 | 后置 |
| RACI（C/I 列） | C/I 不驱动代码 | 降为注释或删除 |

**保留（必要）**：统一 `AIPlanNode`（取代 `AITaskStep` + `AIRfcWorkItem` 投影）、六态、`ParentId` 分组树（聚合）、`DependsOn`（现名）、`RunId` 绑定、`Scope`、`Summary`、fail-closed 聚合、汇总门锁根。

## §2 设计（最小必要版）

### 2.1 节点模型 `AIPlanNode`

一个节点同时是「计划单元」与「状态单元」，字段分三组（**结构 / 状态 / 绑定**）：

| 组 | 字段 | 类型 | 语义 |
|----|------|------|------|
| 结构 | `Id` | `string` | 节点稳定标识（`"n"+Guid`；树内唯一） |
| 结构 | `ParentId` | `string` | 父节点；根为空（根由 `AIPlanTree.Root` 承载） |
| 结构 | `Children` | `List<AIPlanNode>` | 子节点（分解产物） |
| 结构 | `Kind` | `AIPlanNodeKind` | `Leaf`（可执行）/ `Group`（纯聚合） |
| 结构 | `Title` / `Description` / `Files` | `string` | 承接 `AITaskStep` 三字段（`Index` 由树内序派生） |
| 结构 | `DependsOn` | `List<string>` | 跨分支顺序约束（沿用 `AIRfcWorkItem.DependsOn` 现名） |
| 结构 | `Scope` | `List<string>` | 叶的预声明写面；组 = 子树并集（派生，不落盘） |
| 状态 | `Status` | `AIPlanNodeStatus` | 见 §2.3 |
| 状态 | `Summary` | `string` | 叶执行终态必答小结（空 = 无） |
| 绑定 | `RunId` | `string` | 绑定的 `AISubAgentRun.RunId`（`Arc.Agent` 不 import Harness 类型） |

### 2.2 `AIPlanTree`（容器）

```text
AIPlanTree
  ├─ Root: AIPlanNode            // 隐式根（ParentId 空，Kind=Group）
  ├─ RootVerifying: bool         // 根专属 Verifying（不进通用枚举）
  ├─ ComputeStatus(): 自底向上聚合（§2.4）
  ├─ MarkRootVerified(): 根完成（DoD 全勾）——清 Verifying 标志、根落 Completed
  ├─ Validate(): 树结构 Lint（DependsOn 引用 / 非法态）
  ├─ FindNode(id): 按 Id 查找（MarkNodeDone / Validate / DependsOn 判定用）
  └─ FromFlat(steps): 迁移构造（单层树；dod e2e 依赖，按需保留）
```

> `AllNodes` / `Leaves` / `Ready()` 由旧设计暴露为公共 API，现收敛为内部遍历辅助（按需）；拓扑就绪逻辑 P1 不接线，随 P1c 调度单元切换时再补。

### 2.3 节点状态机 `AIPlanNodeStatus`（六态 + 根专属 Verifying）

| 状态 | 适用 | 含义 |
|------|------|------|
| `Pending` | 叶/组 | 未轮到（`DependsOn` 未满或未被扇出窗口选中） |
| `Ready` | 叶/组 | 依赖已满、可派发（等并行度节流） |
| `Running` | 叶/组 | 叶：子代理在飞；组：≥1 子在飞 |
| `Completed` | 叶/组 | 叶：执行层终结（非领域终态）；组：全子 `Completed`（纯聚合） |
| `Failed` | 叶/组 | 叶执行失败 / 组聚合含失败（红吸收向上） |
| `Cancelled` | 叶/组 | 撤单（叶或整枝） |

**根专属 `Verifying`**（不进通用枚举）：当根节点聚合结果为「全叶 `Completed` 且无失败」时，`AIPlanTree.RootVerifying` 置 `true`（对应 `AIPlan.Status = Verifying`，待汇总门 D0–D7）；DoD 全勾后经 `MarkRootVerified()` 清除并置根 `Completed`。组级**无** `Verifying`（全子 `Completed` → `Completed`）。

**叶 → 组 → 根的执行/聚合流**：

```text
Leaf:  Pending ─DependsOn满─► Ready ─Dispatch─► Running ─终态─► Completed / Failed / Cancelled
Group: Pending ──子Ready──► Running(≥1子在飞) ──全子Completed──► Completed
Root:  Pending ──子Ready──► Running(≥1子在飞) ──全子Completed──► Completed + RootVerifying ─DoD全勾─► Completed
```

### 2.4 聚合规则（fail-closed）

按**优先级**从上到下匹配，命中即停（父 = 子态上确界）：

| 优先级 | 条件（子态） | 父态 |
|:---:|------|------|
| 1 | 存在 `Failed` | `Failed` |
| 2 | 存在 `Cancelled` | `Failed`（未完结计入红） |
| 3 | 存在 `Running` | `Running` |
| 4 | 存在 `Ready` 或 `Pending` | `Pending` |
| 5 | 其余（全 `Completed`） | `Completed`（若为根 → 同时 `RootVerifying = true`） |

### 2.5 非法态（结构不变量，`Validate()`）

| # | 非法态 | 判定 |
|---|--------|------|
| I1 | 子 `Failed` 但父非 `Failed` | 非法（红吸收必须逐层上浮） |
| I2 | 子 `Pending`/`Ready`/`Running` 但父 `Completed` | 非法（未派发子不得出现在已完成父下） |
| I4 | 根 `Completed` 但 DoD 未全勾 | 非法（由 `AIPlan.Complete` 受控 API 强制，不在 `Validate()` 内判） |
| I7 | `DependsOn` 引用不存在 Id | 非法（构造期校验） |
| I8 | 叶终态缺 `Summary` | 非法（叶 `Completed`/`Failed`/`Cancelled` 必交小结） |

## §3 API 草图（`AI` 前缀，`Arc.Agent` 归属）

```as
// 归属：Arc.Agent
namespace Arc.Agent;

public enum AIPlanNodeKind {
    Leaf,        // 可执行叶（可委托子代理）
    Group,       // 纯聚合组（无自身执行）
}

public enum AIPlanNodeStatus {
    Pending, Ready, Running,
    Completed,     // 叶：执行层终结；组：全子 Completed（纯聚合）
    Failed, Cancelled,
    // Verifying 为根专属（AIPlanTree.RootVerifying），不进本枚举
}

public class AIPlanNode {
    public string Id { get; set; }
    public string ParentId { get; set; }              // 根为空
    public List<AIPlanNode> Children { get; }         // 分解关系
    public AIPlanNodeKind Kind { get; set; }
    public string Title { get; set; }
    public string Description { get; set; }
    public string Files { get; set; }
    public List<string> DependsOn { get; }            // 跨分支顺序（沿用 AIRfcWorkItem.DependsOn）
    public List<string> Scope { get; }                // 预声明写面
    public AIPlanNodeStatus Status { get; set; }
    public string Summary { get; set; }
    public string RunId { get; set; }                 // 绑定的 AISubAgentRun.RunId

    public bool IsLeaf { get; }
    public bool IsTerminal { get; }
}

public class AIPlanTree {
    public AIPlanNode Root { get; }
    public bool RootVerifying { get; }                // 根专属 Verifying
    public void ComputeStatus();                      // 自底向上聚合（§2.4）
    public void MarkRootVerified();                   // 根完成（DoD 全勾）
    public List<string> Validate();                   // DependsOn 引用 / 非法态 Lint
    public AIPlanNode? FindNode(string id);
    public static AIPlanTree FromFlat(List<AITaskStep> steps); // 迁移：单层树（按需保留）
}
```

`AIPlan` 侧（增量改动，不重写状态机）：

```as
public class AIPlan {
    // ...Goal/Analysis/Verification/Status/Revision 不变...
    public AIPlanTree Tree { get; set; }             // 取代 List<AITaskStep> Steps
    public List<AIPlanNode> Steps { get; }           // = Tree.Root.Children（向后等价投影）
    public int TotalSteps { get; }                   // = Tree.Root.Children.Count
    public int CompletedSteps { get; }               // = Completed 叶计数
    public void MarkNodeDone(string nodeId);         // 叶落终态 → ComputeStatus → RootVerifying → 计划态 Verifying
    public void Complete();                          // 仅 Verifying → Completed（汇总门唯一写入，经 MarkRootVerified）
}
```

## §4 汇总门与树的关系（D0–D7）

**结论：D0–D7 是「根聚合过」，不是「每个叶各过」**（对齐 [parallel-subagents §4.4](parallel-subagents.md)）。叶 `Completed` = 执行层终结（交小结），**≠ 领域终态**；根 `Completed` = 汇总门 D0–D7 全勾后经 `AIPlanGate.CompleteByDoD` 唯一写入（`MarkRootVerified`）。

## §5 演进路径（P1a–P1c，禁双轨，同变更集收口）

| 阶段 | 内容 | 前置 | 验收 |
|------|------|------|------|
| **P1（已具备能力面）** | `AIPlanNode`/`AIPlanTree`/`AIPlanNodeStatus`（六态）落 `Arc.Agent`；`AITaskStep→AIPlanNode`（单层树）；`AIPlan.Tree` + `Steps` 投影；`MarkStepDone→MarkNodeDone`；根专属 `RootVerifying` | — | 存量 e2e（`arc_ai_parallel_*`/`arc_ai_plan_*`/`arc_ai_dod_*`）不回归 |
| **P1a** | `AIRfcWorkItem.DependsOn/Scope` 收编进 `AIPlanNode`；`AIRfc.WorkItems` 退化为 `AIPlanTree` 序列化投影 | P1 | 拓扑依赖/聚合态正反用例绿 |
| **P1b** | 以树为调度单元（`RunTreeAsync` 等）；拓扑就绪（`DependsOn` 满）扇出 | P1a | 场景五面推演闭环 |
| **P1c** | 动态派发 + 预算收束完整版 | P1b | 场景五面推演闭环 |

每阶段「完成」= 该步场景五面推演闭环（[scenario-drive-acceptance](scenario-drive-acceptance.md)）+ 现有 e2e 不回归，且**同变更集改名 + 改调用点**，不留旧类型。

## §6 设计评审自评 + 诚实边界

| 维度 | 自评 |
|------|------|
| **收敛** | 单一树结构取代 `AITaskStep` 扁平列表与重复投影；六态 + 根专属 `Verifying` 取代九态；`DependsOn` 沿用现名不另造 `OrderAfter`；归属只留 `RunId`（owner 恒 = 主代理，无多级委托） |
| **极简** | `AllNodes`/`Leaves`/`Ready()` 收敛为内部按需遍历，不暴露为公共 API；不设 `Checkpoint`/`Skipped`/`EffectiveScope` 等无消费方抽象 |
| **模块化** | 职责单向：`AIPlanNode`（结构+状态，`Arc.Agent`）→ 调度（`Arc.Agent.Harness`）→ 汇总门（`Coding`）；`Arc.Agent` 只存 `RunId` 字符串引用不 import Harness，依赖无环 |
| **零冗余** | 消除 `WorkItemId=Id` 迁移别名、`DelegatedSessionId`↔`RunId` 1:1、`OrderAfter`↔`DependsOn` 换名三处语义冗余 |

**诚实的边界（不宣称）**：本设计为**最小必要版**；**P1（单层树 + 聚合）已具备能力面**，但 P1a（`DependsOn`/`Scope` 收编）/ P1b（调度单元切换）/ P1c（动态派发）未实现前，不得宣称「AIPlan 多级树化完成」（对齐 043 宣称纪律）。

---

[返回 043(../../043-harness.md) · [AIRfc](airfc.md) · [并行子代理（P3）](parallel-subagents.md) · [子代理管理（方案 A）](subagent-management.md) · [references 索引](index.md)
