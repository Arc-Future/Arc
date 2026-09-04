# 包布局（Harness 基座 · Coding）

> 关联 [043(../../043-harness.md) §10 · §11；类型草图见 [api-sketch](api-sketch.md)；迁移步骤见 [convergence-migration](convergence-migration.md)。本子项锁定目录树、`arc.toml` 依赖边与从基座迁出 Coding 的文件清单。首切片 `std/AI/Agent.Harness` 为试探，**不是**终态包边界。

## §0 宣称门闩

| 宣称 | 条件 |
|------|------|
| 「Coding 包已拆出」 | 存在 `std/AI/Agent.Harness.Coding/` + 独立 `arc.toml`；`quality.*` 与 D0–D7 **判定**已迁入；基座不再直接依赖 `arc` CLI 跑门 |
| 「包布局终态」 | 依赖边与本文一致，且 [收敛迁移](convergence-migration.md) M7 / M9 验收通过 |
| 「仅改 namespace 即算拆包」 | **禁止**——物理目录、包名、依赖边、终端组装须同时对齐 |

未完成对应迁移步时，**禁止**宣称「Harness / Coding 分层已落地」。

## 1. 目标目录树

### 1.1 `std/AI/Agent.Harness/`（基座）

```text
std/AI/Agent.Harness/
├── arc.toml
├── Rfc/
│   ├── AIRfc.as              # 聚合根 + Intention/Design/Acceptance Spec 面
│   ├── AIRfcWorkItem.as
│   └── AIRfcRuntime.as       # Create / Revise / AttachPlan / BindWorkItem
├── DoD/
│   ├── AIDoDGate.as            # AIDoDGateKind / AIDoDGateStatus / AIDoDGateResult
│   ├── IAIDoDGateEvaluator.as  # 领域判定注入点（无 arc CLI）
│   └── AIDoDOrchestrator.as    # 编排骨架；委托 IAIDoDGateEvaluator
├── Summary/
│   └── AIWorkSummary.as
└── Host/                     # 可选：薄辅助；禁永久 HarnessEventLog
    └── （无独立事件日志库；轨迹写 Agent 会话事件）
```

**基座不得包含**：`Quality/`、`Events/HarnessEventLog`、平行 `PlanSpec`、写死 `arc build`/`arc test` 的判定体。

### 1.2 `std/AI/Agent.Harness.Coding/`（领域一）

```text
std/AI/Agent.Harness.Coding/
├── arc.toml
├── Quality/
│   ├── QualityCli.as         # 进程调用 arc CLI（从基座迁入）
│   └── QualityTools.as       # [AITool] quality.*（从基座迁入）
├── DoD/
│   └── CodingDoDGateEvaluator.as  # IAIDoDGateEvaluator：D0–D7 Coding 信号
├── （Skills/ conventions 约定模板归 crates/arc/templates/，由 arc CLI 内嵌分发，库包内不设 Skills 资源目录）
└── Checkpoints/              # Coding 协作确认点清单（按需）
```

## 2. `arc.toml`：name / namespace / 依赖边

### 2.1 基座

```toml
# std/AI/Agent.Harness/arc.toml
[package]
name = "Arc.Agent.Harness"
edition = "1"
namespace = "Arc.Agent.Harness"

[dependencies]
Arc = { path = "../../Arc" }
"Arc.Agent" = { path = "../Agent" }
```

- **依赖**：仅 `Arc` + `Arc.Agent`（消费 `AIPlan` / `AICoordinator` / 会话事件）。
- **禁止**：依赖 `Arc.Agent.Harness.Coding`（基座不反向依赖领域）。

### 2.2 Coding

```toml
# std/AI/Agent.Harness.Coding/arc.toml
[package]
name = "Arc.Agent.Harness.Coding"
edition = "1"
namespace = "Arc.Agent.Harness.Coding"

[dependencies]
Arc = { path = "../../Arc" }
"Arc.Agent" = { path = "../Agent" }
"Arc.Agent.Harness" = { path = "../Agent.Harness" }
```

- **依赖**：`Arc.Agent`（工具/会话）+ `Arc.Agent.Harness`（DoD 骨架 / AIRfc 类型）。
- **职责**：`quality.*`、D0–D7 **判定**、协作清单（默认 conventions 模板已迁 `crates/arc/templates/conventions.agent.md`，经 `include_str!` 由 arc CLI 内嵌分发，`arc new --agent` 落盘 `.arcagent/conventions.md`）。

### 2.3 依赖边（总览）

```text
Arc.Agent
    ↑
Arc.Agent.Harness ──────────────┐
    ↑                           │
Arc.Agent.Harness.Coding ───────┘
    ↑
examples/ArcAgent（及同类终端工程）
```

## 3. `examples/ArcAgent` 应依赖的包

终态薄组装根须显式依赖：

| 包 | 是否依赖 | 用途 |
|----|----------|------|
| `Arc.Agent` | ✅ | 宿主 / 会话 / Plan / HITL |
| `Arc.Agent.DeepSeek`（或其它 Provider） | ✅（按产品） | 推理 Provider |
| `Arc.Agent.Harness` | ✅ | AIRfc 运行时 + DoD 骨架 + 小结 |
| `Arc.Agent.Harness.Coding` | ✅ | `quality.*` + Coding 门判定 |

```toml
# examples/ArcAgent/arc.toml（目标态示意）
[dependencies]
"Arc.Agent" = { path = "../../std/AI/Agent" }
"Arc.Agent.DeepSeek" = { path = "../../std/AI/Agent.DeepSeek" }
"Arc.Agent.Harness" = { path = "../../std/AI/Agent.Harness" }
"Arc.Agent.Harness.Coding" = { path = "../../std/AI/Agent.Harness.Coding" }
```

组装纪律：只**接线**（注册 `CodingDoDGateEvaluator`、挂 `QualityTools`、创建 `AIRfcRuntime`），**禁止**在 example 内再实现 PM / 质量门 / 第二套事件日志。

### 3.1 领域二样例：`examples/ReviewAgent`（基座复用性验证）

领域二 = **数据/文档审查型 Harness**（RFC 043 领域二样例）：证明「只组装 Agent + 基座 + 领域工具」即可建成新领域，且**零触碰基座**。

```toml
# examples/ReviewAgent/arc.toml —— 不含 Arc.Agent.Harness.Coding
[dependencies]
"Arc.Agent" = { path = "../../std/AI/Agent" }
"Arc.Agent.DeepSeek" = { path = "../../std/AI/Agent.DeepSeek" }
"Arc.Agent.Harness" = { path = "../../std/AI/Agent.Harness" }
```

| 面 | 落点 | 说明 |
|----|------|------|
| 领域工具 | `ReviewAgent/Tools/ReviewTools.as` | 声明式 `[AITool]`：`review_file` / `check_consistency`（能力 `review.Run`），编译期自动装配 |
| 领域判定 | `ReviewAgent/DoD/ReviewDoDGateEvaluator.as` | 实现基座 `IAIDoDGateEvaluator`：D0 = 文档集完备、D3 = 交叉引用一致性（真实信号可证伪）；未接线门诚实 Pending |
| 组合根 | `ReviewAgent/Program.as` + `Host/ReviewHost.as` | 复用 `AIHarnessSession`（AIRfc / DoD / 事件单轨），仅注入领域 evaluator + 领域工具 |
| 领域提示 | `ReviewAgent/Prompt/ReviewAgentPrompt.as` | 文档审查 Agent 系统指令（无 Coding 编程指令） |

**领域二复用能力面**：

1. 基座不依赖 Coding：能力白名单由终端工程自行声明，基座不含 Coding 耦合。
2. `examples/ArcAgent`（Coding 域）与 `examples/ReviewAgent`（领域二）分列两端：基座改动只去耦合、不改行为面。
3. 无 Coding 依赖的项目可自动装配领域 `[AITool]`、经 `AIHost` 真实执行、复用 `AIHarnessSession` 跑真实 D0/D3 门。
4. **诚实边界**：基座 `AIDoDOrchestrator.AllPassed` 完成策略为 D0–D7 全 Passed；非 Coding 领域不适用门（如 ReviewAgent 的 D1/D2/D4/D6）保持 Pending → `Completed` 不假绿。领域化完成策略（仅要求适用门全过）为未来基座扩展项，不属「零触碰基座」约束内。

依赖边（领域二样例挂接，跨 Coding 平行）：

```text
Arc.Agent
    ↑
Arc.Agent.Harness ─────────────┐
    ↑                          │
Arc.Agent.Harness.Coding ──────┘    examples/ReviewAgent（领域二，不依赖 Coding）
    ↑                                  ↑
examples/ArcAgent（领域一 Coding）      （领域三…平行挂接）
```

## 4. 现有文件：基座 → Coding 迁移清单

首切片落在 `std/AI/Agent.Harness/` 且**必须迁出**的文件：

| 现路径（试探） | 目标包 | 说明 |
|----------------|--------|------|
| `Quality/QualityTools.as` | `Arc.Agent.Harness.Coding` | `quality.arc_build` / `arc_test` / `arc_check` / `arcgr_query` |
| `Quality/QualityCli.as` | `Arc.Agent.Harness.Coding` | 进程调用 `arc` CLI |
| `DoD/AIDoDOrchestrator.as` 中写死 `QualityCli.RunArcAsync(...)` 的判定体 | 抽到 Coding 的 `CodingDoDGateEvaluator` | 基座 `AIDoDOrchestrator` 只保留编排 + `IAIDoDGateEvaluator` 委托 |
| （未来）契约扫描 / `.arcgr` 查询辅助 | Coding | D1/D2/D4/D6 信号同源 |

**留在基座**：

| 现路径 | 终态处理 |
|--------|----------|
| `DoD/AIDoDGate.as` | 保留为门骨架 |
| `DoD/AIDoDOrchestrator.as` | 去掉 arc CLI 硬编码后保留编排 |
| `Summary/AIWorkSummary.as` | 保留 |
| `Anchor/HarnessAnchor.as` / `PlanSpec` | **删除/替换**为 `Rfc/AIRfc*`（见迁移 M4） |
| `Events/HarnessEvent.as`（含 `HarnessEventLog`） | **删除**；轨迹改写 Agent 会话事件（见迁移 M5–M6） |
| `Host/AIHarnessSession.as` | 收敛为薄组装或下沉 example；不把 quality 能力焊在基座会话壳 |

## 5. 与架构锁的对照

| 架构锁 | 本布局落点 |
|--------|------------|
| Arc.Agent → Harness → Coding | 依赖边单向，基座不依赖 Coding |
| Plan 面 = AIPlan 引用 | 基座 `AIRfc` 无 `PlanSpec` |
| quality.* / D0–D7 判定在 Coding | `Quality/` + `CodingDoDGateEvaluator` |
| 基座只有门骨架 | `AIDoDGate*` + `AIDoDOrchestrator` + `IAIDoDGateEvaluator` |
| 事件入 Agent 会话 | 无 `Events/HarnessEventLog` |
| 首切片为试探 | 上表「现路径」均为过渡，不得当终态目录冻结 |

---

[返回 043(../../043-harness.md) · [API 草图](api-sketch.md) · [收敛迁移](convergence-migration.md) · [references 索引](index.md)
