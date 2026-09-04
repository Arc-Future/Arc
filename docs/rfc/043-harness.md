# RFC 043 Coding Agent Harness 工程

## 读前门闩（写代码前）

未勾选下列全部项前，**禁止**为实现 Harness / AIRfc / Coding 领域写业务代码（含「临时对齐旧切片」的扩写）。细则见 [llm-gates](043-harness/references/llm-gates.md)。

- [ ] 已读 [AIRfc 体系](043-harness/references/airfc.md)（宣称门闩、类型契约、双 Revision、禁令）
- [ ] 已读 [LLM 门闩](043-harness/references/llm-gates.md)（何时可写 / 不可写代码）
- [ ] 已理解分层：`Arc.Agent` → `Arc.Agent.Harness` → `Arc.Agent.Harness.Coding`（见 §10）
- [ ] 已确认 Plan 面 = **`AIPlan` 引用**，禁止平行 `PlanSpec`
- [ ] 已确认决策轨迹写入 **Agent 会话事件**，禁止永久 `HarnessEventLog`
- [ ] 已确认冲突织物由宿主统一提供，`AIRfc` / `AIPlan` / `AITool` 共用（见 [conflict-fabric](043-harness/references/conflict-fabric.md)）
- [ ] 已确认禁止引入历史试探形态（`HarnessAnchor` / `PlanSpec` / `HarnessEventLog` 等），不得回潮、不得当终态抄（见 §11）
- [ ] 宣称「完成 / Completed / 终态」前已对照 [宣称纪律](#宣称纪律) 与 [airfc §0](043-harness/references/airfc.md)

## 宣称纪律

学 [RFC 036 §4](036-maturity.md)：「未经验收不得宣称；验收通过后方可声明」。对本 RFC：

| 条款 | 内容 |
|------|------|
| 不得宣称 | 未满足 DoD / 未过 [llm-gates](043-harness/references/llm-gates.md) / 仍依赖试探类型时，禁止宣称「完成」「Completed」「终态」「已收敛」 |
| `AIPlan.Completed` | 仅当 Coding 领域 D0–D7 **全勾**（见 [definition-of-done](043-harness/references/definition-of-done.md)） |
| AIRfc 终态切片 | 仅当聚合根契约、双 Revision、冲突织物消费、事件轨迹均按 [airfc 验收 DoD](043-harness/references/airfc.md) 绿 |
| 宣布主体 | 维护者书面宣布，并引用可复测证据窗（测试 / e2e / 门闩清单） |
| 撤回 | 后续回归 → 声明自动失效，修复后再宣布 |

**替代说法**：未过门闩则只可写「试探 / 进行中 / 未验收」；禁止用完成语掩盖未对齐。

## 背景

Coding Agent Harness 是面向 **Arc 开发者用户**的编码智能体**领域工程**：开发者给出需求，经方向环确立目标、执行环实现验证，交付可执行、可验收、符合工程标准的代码。它**不是** Arc 维护者（编译器 / 标准库作者）的内部工具，而是随生态分发、开箱即用的领域标准之一。

**分层总览**（跨领域可复用）：

```text
Arc.Agent                         ← 通用宿主基础设施（038）
  └─ Arc.Agent.Harness            ← Harness 基座：AIRfc + 双环协议 + DoD 骨架
        └─ Arc.Agent.Harness.Coding ← 领域一：Coding（D0–D7 信号 / quality.* / 默认 Skills）
              （未来领域二、三…同级挂接，不焊进基座）
```

Harness 基座构建于 [038 AI 宿主](038-ai-host.md) 之上；Coding 领域另消费编译管线结构化诊断（[013](013-compiler-pipeline.md)）、`.arcgr`（[034](034-ai-toolchain-arcgr.md)）、QIF（[032](032-qif.md)）。

**心智模型**：难点不是「怎么做」，而是「做什么、做成什么样」。高质量来自**方向确认 + 验证闭环**。总纲为**双环 + AIRfc + 纠偏**：方向环（人）回答「要什么」，执行环（机器）回答「怎么对」；**AIRfc** 是跨任务、跨版本流转的小型项目管理 / 需求本尊体系；纠偏是对 AIRfc 的增量升版。

### 渐进式披露（references）

| 子项 | 内容 | 边界（不在此篇） |
|------|------|----------------|
| [AIRfc 体系](043-harness/references/airfc.md) | 小型 PM 运行时、Spec 面、跨任务/跨版本、与 AIPlan 复用、冲突织物 | AIPlan/HITL 机制见 038 |
| [LLM 门闩](043-harness/references/llm-gates.md) | 写代码前硬门闩、宣称禁令 | 语言/编译门见 013 |
| [冲突织物](043-harness/references/conflict-fabric.md) | AIRfc / AIPlan / AITool 共用租约 | 宿主协调面细节见 038 |
| [API 草图](043-harness/references/api-sketch.md) | 聚合根与包面公开签名草图 | 实现见 package-layout |
| [包布局](043-harness/references/package-layout.md) | `Arc.Agent.Harness` / `.Coding` 目录与职责 | 收敛步骤见 migration |
| [收敛迁移](043-harness/references/convergence-migration.md) | 试探 → 终态迁移清单 | 试探禁抄见 §11 |
| [可执行 DoD 与验证闭环](043-harness/references/definition-of-done.md) | D0–D7（Coding 领域判定）、L2/L3 边界、绿点 | AIRfc/纠偏见 airfc；编译见 013 |
| [纠偏协议](043-harness/references/anchor-correction-protocol.md) | 修复/纠偏分流、偏差检测、决策轨迹 | 事件日志承载见 038 |
| [设计先行与设计评审](043-harness/references/design-review.md) | 远见/收敛/模块化/零冗余 | plan 门闩见 038 |
| [协作确认点](043-harness/references/collaboration-checkpoints.md) | 高沟通价值决策点 | HITL 见 038 §5 |
| [测试先行与纠偏同步](043-harness/references/testing-first.md) | 验收测试锁定；随 AIRfc 升版同步 | QIF 见 032 |
| [回合小结与偏差判定](043-harness/references/work-summary.md) | 每单元小结格式与偏差判定 | 偏差触发见纠偏协议 |

## 设计决策

### 1. 定位与心智模型

| 面 | 正道 | 拒绝 |
|----|------|------|
| 用户（Coding 领域） | Arc 开发者（写 `.as`、跑 `arc build` / 集成测试） | 仅服务维护者的内部工具 |
| 心智模型 | 双环 + **AIRfc** + 纠偏 | 纯验证驱动；或以散落 Spec 字段冒充需求体系 |
| 协作 | Agent 是员工，关键点对人 | 每步问人；或完全无人值守 |
| 质量来源 | 方向确认 + 验证闭环 | 依赖模型生成质量 |
| 复用纪律 | 构建 Harness 时优先用尽 `Arc.Agent` 已有能力 | 平行再造 Plan / HITL / 事件 / 冲突机制 |

### 2. AIRfc 体系（需求本尊 · 小型项目管理运行时）

**`AIRfc`**（读作 AI RFC）：Harness 基座内集成的**小型项目管理体系**——任务级需求本尊 + 跨任务、跨版本流转；不是企业级 PLM，也不是聊天里的散落说明。

| 面 | 契约 |
|----|------|
| 定位 | 方向环与执行环的**唯一事实源**；纠偏只升版 AIRfc，不散落于对话 |
| 范围 | **跨任务、跨版本**运作（多工作项并行、Revision 轨迹可审计） |
| Spec 聚合 | 意图 / 设计 / 验收为 **AIRfc 内部面**（历史过渡称呼见 [airfc §8](043-harness/references/airfc.md)；正道产品名一律 **AIRfc**） |
| 计划面 | **直接复用** [`AIPlan`](038-ai-host.md)（含 PlanGate）；**禁止**平行 `PlanSpec` / 二次包装 |
| 命名消歧 | `AIRfc` = 本体系工件与运行时；与仓库 `docs/rfc/` 设计文档正交 |

必填 Spec 面（聚合在 AIRfc 内，非四个平行模块）：

| Spec 面 | 内容 |
|---------|------|
| 意图 Intention | 用户要什么可感知结果（非技术细节） |
| 设计 Design | 远见 + 收敛 + 结构 + 模式 +「为什么」 |
| 验收 Acceptance | 可执行场景 + 断言（测试先行锁定） |
| 计划 Plan | = **`AIPlan` 引用**（步骤 + 验证点 + 门闩批准） |

完整运行时、双 Revision、冲突织物见 [airfc](043-harness/references/airfc.md)。

### 3. 执行环与可执行 DoD（Coding 领域）

D0–D7 的**门骨架**在 Harness 基座；**判定信号**（`arc build` / `.arcgr` / 契约扫描等）在 **`Arc.Agent.Harness.Coding`**。

| 门 | 判定 | 自动化 |
|----|------|--------|
| D0 编译门 | `arc build` 0 error（**warning 面未判定**，见 [DoD D0 行](043-harness/references/definition-of-done.md)） | 全自动 |
| D1 语义完整性门 | `.arcgr` 引用/契约/可达性全绿 | 全自动 |
| D2 契约硬规则门 | 编码契约机器可查项全过 | 全自动 |
| D3 行为验证门 | 验收测试全绿（防降级） | 自动 + 边界升级 |
| D4 diff 覆盖门 | 改动对 AIPlan 覆盖、无越界 | 全自动 |
| D5 自审收敛门 | 每条 acceptance 有可执行证明 | 自动 + 人抽查 |
| D6 反模式门 | 反模式可查项全过 | 全自动 |
| D7 人验收门 | 协作确认点经人确认 | 人 |

`AIPlan` 状态 `Completed` 的可执行定义 = D0–D7 全勾。细则见 [definition-of-done](043-harness/references/definition-of-done.md)。宣称该状态须遵守上文[宣称纪律](#宣称纪律)。

### 4. 纠偏协议

支柱：**AIRfc 唯一、增量升版（非推翻）、修复/纠偏分流、决策轨迹入 Agent 事件日志**。见 [纠偏协议](043-harness/references/anchor-correction-protocol.md)。

### 5–6. L2 / L3 自动化边界

同前：验证失败自动迭代 ≤3 轮；机器筛平凡项，人审高沟通价值点。见 [definition-of-done](043-harness/references/definition-of-done.md) 与 [collaboration-checkpoints](043-harness/references/collaboration-checkpoints.md)。

### 7. 设计先行

设计是 AIRfc Spec 面之一；评审清单见 [design-review](043-harness/references/design-review.md)。

### 8. 协作确认点

高风险 = 高沟通价值决策点。清单见 [collaboration-checkpoints](043-harness/references/collaboration-checkpoints.md)。

### 9. 测试先行与纠偏同步

测试属于 AIRfc 验收面；纠偏升版时测试同步。见 [testing-first](043-harness/references/testing-first.md)。

### 10. 落地归属（Agent · Harness 基座 · 领域 Harness）

| 层 | 允许 | 禁止 |
|----|------|------|
| `Arc.Agent`（038） | 会话 / `[AITool]` / HITL / Wiki / Context / CodeAct / MCP；**`AIPlan` + PlanGate + `AITaskRun`**；**冲突织物**（`AICoordinator` 升维，供 AIRfc / AIPlan / AITool 共用）；会话事件轨迹 | 领域 DoD 判定、Coding 专用 `quality.*`、第二套 Plan/锁 |
| `Arc.Agent.Harness` | **`AIRfc` 运行时**（跨任务/跨版本）；Spec 聚合；纠偏协议；回合小结；DoD **门骨架**；验证器工具的贡献约定（只读、不进 plan gate） | 平行 `PlanSpec`；永久 `HarnessEventLog`；焊死 Coding 信号；私有冲突锁 |
| `Arc.Agent.Harness.Coding` | `quality.*`（`arc_build` / `arc_test` / `arc_check` / `arcgr_query`）；D0–D7 **Coding 判定**；默认 conventions / Skills / 协作清单 | 第二套 AIRfc；绕过 HITL/PlanGate；自造事件库 |
| 终端工程（如 `examples/ArcAgent`） | 薄组合根：组装 Agent + Harness + Coding | 重复实现 PM / 质量门 / 冲突织物 |

**复用纪律（硬约束）**：若搭领域 Harness 时必须平行再造 Agent 已有能力（Plan / HITL / 事件轨迹 / 冲突处理），优先改 **Agent**，禁止在 Harness 包一层同名物。

**冲突织物（硬约束）**：`AIRfc` + `AIPlan` + `AITool` 共用同一套跨会话、多任务并行冲突处理（租约 / 授予 / 冲突检测 / 原子提交）；禁止三套各搞各的锁。详见 [conflict-fabric](043-harness/references/conflict-fabric.md) 与 [038](038-ai-host.md)。

包面与迁移见 [package-layout](043-harness/references/package-layout.md)、[api-sketch](043-harness/references/api-sketch.md)、[convergence-migration](043-harness/references/convergence-migration.md)。

### 11. 禁止项（试探形态不得回潮）

下列历史试探形态**不得**重新引入、抄写、扩展或对外宣传（回潮即双轨）：

- `HarnessAnchor`：散锚点，禁止；
- `PlanSpec`：平行 Plan 结构，禁止——Plan 面 = **`AIPlan` 引用**（[038](038-ai-host.md)）；
- `HarnessEventLog`：独立决策日志，禁止——决策轨迹一律写入 **Agent 会话事件**；
- 焊在基座的 `quality.*`：禁止——`quality.*` 与 D0–D7 判定归属 **`Arc.Agent.Harness.Coding`**。

同时禁止：以试探类型通过率冒充「Harness 完成 / Completed / 终态」宣称；在未读 [llm-gates](043-harness/references/llm-gates.md) 与 [airfc](043-harness/references/airfc.md) 的情况下，以「兼容旧切片」为由扩大试探面。

## 边界

- **Arc.Agent 宿主**见 [038](038-ai-host.md)；纯推理见 [041](041-ai-inference.md)。
- **`.arcgr`** 见 [034](034-ai-toolchain-arcgr.md)；编译诊断见 [013](013-compiler-pipeline.md)。
- **QIF** 见 [032](032-qif.md)。
- **AIRfc 细节**见 [airfc](043-harness/references/airfc.md)；本 RFC 只定架构级决策。
- **写代码门闩**见 [llm-gates](043-harness/references/llm-gates.md)。

---
上一节：[042 P2P 网络](042-p2p.md) · 下一节：（未分配）
