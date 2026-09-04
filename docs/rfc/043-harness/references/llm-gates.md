# LLM 宣称门闩（开工 / 合入）

> **注（2026-08-29）**：原 `crates/arc-integration` 已退场（a2627a0f）。本文所引
> `cargo test -p arc-integration ...` 验证命令与 `crates/arc-integration/tests/` 路径
> （含 §G-M8 的 `rg ... crates/arc-integration/tests/arc_ai_*` 示例）不再可用；
> 现行验证矩阵为 `cargo test --workspace`（运行时面
> `cargo test -p arc-tests --features full-rt`），详见仓库根 `CHANGELOG.md`。

> 关联 [043 Coding Agent Harness 工程(../../043-harness.md) §1 · §10 · §11。本子项给出 **Agent / 人**在动手与合入前必须勾选的宣称门闩；与 [可执行 DoD](definition-of-done.md)、[AIRfc](airfc.md) 交叉引用。未勾门闩 = 禁止动手 / 禁止宣称完成。

## §0 宣称

| # | 门闩 |
|---|------|
| 1 | **未勾开工检查单（G-S0…）→ 禁止动手**（含写代码、改 Spec 假绿、宣称「已开工」） |
| 2 | **未勾合入检查单（G-M0…）→ 禁止宣称完成 / 禁止合入** |
| 3 | **终态宣称**仅在合入检查单全勾且收敛方向落地后允许；试探切片（平行 Plan / 双轨事件 / 基座焊 `quality.*`）**禁止**称为终态 |

## 开工检查单（G-S0…）

动手前逐项勾选；缺一不得开工。

```
□ G-S0  已读 043 分层：Arc.Agent → Arc.Agent.Harness → Arc.Agent.Harness.Coding（三层包名清晰）
□ G-S1  需求本尊 = AIRfc；Plan 面 = AIPlan 引用（禁平行 PlanSpec / 二次包装）
□ G-S2  事件轨迹走 Agent 会话事件（禁新建独立 HarnessEventLog 作为正道）
□ G-S3  quality.* / D0–D7 判定信号归属 Coding 领域（禁焊进 Harness 基座当终态）
□ G-S4  当前 AIRfc Revision 已知；执行环只认最新版
□ G-S5  冲突织物：AIRfc / AIPlan / AITool 共用一套锁（禁第二把锁设计）
□ G-S6  已链 DoD：Completed ⇔ D0–D7 全勾（见 definition-of-done）
```

## 合入检查单（G-M0…）

合入 / 宣称完成前逐项勾选；缺一禁止 `Completed`。

```
□ G-M0  无平行 Plan（无 PlanSpec / 第二状态机冒充 AIPlan）
□ G-M1  无双轨事件（无永久独立 HarnessEventLog；轨迹入 Agent 会话事件）
□ G-M2  无第二把锁（冲突只消费统一织物，不平行实现）
□ G-M3  quality.* 不在基座冒充领域判定（Coding 归属可证）
□ G-M4  D0–D7 证据齐全（涉真实外部系统另经 D8 真实接入冒烟门；见 DoD；未接线门保持 Pending，禁 Passed 假绿）
□ G-M5  AIRfc Revision 轨迹可审计（created / revised / rejected 等）
□ G-M6  回合小结已按 work-summary 产出；偏差已判定或已升级
□ G-M7  禁宣称终态，除非：平行结构已删、事件单轨、quality 在 Coding、G-M0–G-M8 全勾
□ G-M8  宣称核对：逐条「已实现 / 已支持 / 已修 / 全绿」跑一次可执行核对命令（`rg` / `arc` / `git`）并留证据；未过禁宣称（见下）
```

## 宣称核对（G-M8 · 可执行命令）

> **G-M8 宣称核对**：合入 / 宣称完成前，逐条「已实现 / 已支持 / 已修 / 全绿」须**跑一次可执行核对命令**并留证据（命令 + 命中结果）。命中 ≠ 自动否决，但**有命中未解释 / 未消除 / 无证据 → 禁止宣称**。核对命令随实现收敛调整，下列为基线示例：

```bash
# 平行 Plan 结构
rg -n "PlanSpec" std/AI examples/

# 独立事件日志双轨
rg -n "HarnessEventLog" std/AI examples/

# quality 焊在基座（应落在 Coding）
rg -n "quality\.(arc_build|arc_test|arc_check|arcgr_query)" std/AI/Agent.Harness/
# 期望正道落点示意（路径随实现收敛调整）
rg -n "quality\." std/AI/Agent.Harness.Coding/

# 宣称「已修」的编译器缺陷：核对状态是否已从 plan.md CD 表清除（或标注 ✅ 已修 + 验收 e2e）
rg -n "TODO|todo!\(\)|NotImplemented" std/AI crates/arc-integration/tests/arc_ai_*

# 宣称「全绿」的构建/测试：跑真实命令留 trace（非仅凭记忆）
#   cargo test -p arc-integration --test <e2e>      （管线）
#   cargo run -p arc -- build examples/ArcAgent      （端到端）
#   git status --porcelain -- .                        （改动集核对）

# 宣称「已删除收敛」的类型：全库零残留核对
rg -n "HarnessAnchor|PlanSpec|HarnessEventLog" std examples crates
```

| 命中 | 解读 |
|------|------|
| `PlanSpec` | 平行计划面 → 对照 G-S1 / G-M0 |
| `HarnessEventLog` | 事件双轨 → 对照 G-S2 / G-M1 |
| 基座内 `quality.*` | 领域信号焊错层 → 对照 G-S3 / G-M3 |

> **G-M8 证据要求**：每条「已实现 / 已支持 / 已修 / 全绿」宣称须附**可执行核对命令 + 命中结果**（如 `rg -n "PlanSpec" std examples` → 0 命中；`cargo test -p arc-integration --test <e2e>` → 全绿；`git status --porcelain` → 改动集对齐）。无证据 = 未过 G-M8 → 禁止宣称。对照 [harness-llm-lessons](harness-llm-lessons.md)（B2 机制 · P0-2）。

## 与 DoD / AIRfc 的关系

| 面 | 契约 |
|----|------|
| DoD | `AIPlan` / 任务 `Completed` ⇔ [D0–D7 全勾 + 涉真实外部系统经 D8 真实接入冒烟门](definition-of-done.md)；本篇 G-M4 要求证据，不替代门判定 |
| AIRfc | 开工须知 Revision（G-S4）；合入须轨迹（G-M5）；纠偏见 [纠偏协议](anchor-correction-protocol.md) |
| 043 | 分层与收敛见 [043(../../043-harness.md) §10 · §11；本篇是宣称纪律的可勾清单 |

---

[返回 043(../../043-harness.md) · [AIRfc](airfc.md) · [DoD](definition-of-done.md) · [references 索引](index.md)
