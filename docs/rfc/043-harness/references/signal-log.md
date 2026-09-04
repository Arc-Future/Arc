# 信号日志与 LLM 上下文筛选（AISignalLog · AIToolOutput）

> 关联 [043 Coding Agent Harness 工程(../../043-harness.md) §3 · §10；承接 [真实场景运转协议](scenario-operation.md)（B7 性能与可观测性门 · A.7 非功能需求进验收 · B11 AI 能力边界）、[回合小结](work-summary.md)、[LLM 门闩](llm-gates.md)。本子项是 043 体系的**能力子项**（日志分级 / 筛选 / LLM 上下文预算 / 工具输出门面），与 [性能观测与性能信号](performance-observability.md) 同组落盘，共享演进 P1–P3 节奏。
>
> **性质**：**P1 已具备能力面**：`AISignalLog` 分级落盘面已具备（见 §P1 落地面）；`AISignalEntry` / `BuildLlmView(tokenBudget)` / `AIToolOutput` 等为 P2 设计态（诚实缺口），**不得**据此宣称能力「完成 / Completed / 已收敛」。落地按演进 P1–P3 逐阶段排期（plan.md 登记为 `PF-1–PF-3`），每阶段以 [场景闭环推演验收协议](scenario-drive-acceptance.md) 五面推演闭环为判据。

## §0 宣称门闩

| # | 门闩 |
|---|------|
| 1 | **P1 已具备能力面，P2 为设计态**：`AISignalLog`（分级落盘面）与 `AISignalLevel` 已具备（见 §P1 落地面）；`AISignalEntry` / `BuildLlmView(tokenBudget)` / `AIToolOutput` 为 P2 设计态**未实现**；禁止把设计态写成已实现、禁止据此宣称日志/筛选能力「完成」 |
| 2 | **防双轨（M6 纪律）**：`AISignalLog` 只承载**分级日志与可审计落盘**，**不承载决策轨迹**——决策轨迹仍走 Agent 会话事件单轨（`AIDecisionEventKind`，[llm-gates](llm-gates.md) G-M1）；禁止以日志库冒充事件库 |
| 3 | **分层归属**：基座持骨架（`AISignalLog` / `AISignalEntry` / `AISignalLevel` 类型与落盘面）；Coding 持筛选/判定规则（KeySignal 筛选规则、`AIToolOutput` 门面、perf 阈值）；禁止在基座焊死 Coding 规则 |
| 4 | **字符代理诚实标注**：`BuildLlmView(tokenBudget)` 的 token→字符换算为**近似代理**，非 token 级精确计量；禁止冒充精确 |
| 5 | **演进逐阶段验收**：P1→P3 每阶段验收 = 所属场景（B7 / A.7 / 4.1）五面推演闭环，非测试全绿 |
| 6 | **落盘合规**：日志只落 `target/scratch/arc-logs/`（`.gitignore` 已含 `/target/`），禁写源码树（工作区卫生纪律） |

## 目标

- 让 Harness 的工具 / 门执行有**分级可审计日志**（Debug / Info / Warn / Error），全量落盘不丢失审计面。
- 让**进 LLM 上下文的输出受预算约束**：`BuildLlmView(tokenBudget)` 按字符代理约束筛选折叠，Error / KeySignal 强制进摘要，Debug / Info 只落盘。
- 提供 **`AIToolOutput` 工具输出门面**（Coding）：`exit + 摘要 + perf 摘要 + 日志路径引用`，替代全量输出进 LLM——上下文预算面（B11 超时/预算护栏的「上下文长度」维度）。
- 与 **`AITurnRunner`**（038 回合循环）和 **`QualityTools`**（Coding）接线，成为工具输出进 LLM 的通用门面。

## 非目标

| 项 | 说明 |
|----|------|
| 决策轨迹 | 不替代 Agent 会话事件单轨（M6 防双轨）；`airfc:*` / `checkpoint:*` / `perf:anomaly` 等事件不落日志库 |
| 结构化日志平台 | 不做聚合检索 / 全文索引平台（Seq / ELK 类能力不在范围） |
| token 级精确计量 | `BuildLlmView` 为字符代理约束，非 tokenizer 级精确；精确计量留待模型侧 / 038 预算面 |
| 日志轮转治理 | 保留策略 / 轮转 / 配额为 P3 后评估项（P1–P3 只保证落盘位置合规） |
| 会话回合预算 | 不替代 `AISession` 回合预算（038 §14）——正交：本面管上下文**长度**，回合预算管**轮数** |

## 类型契约与分层归属

```text
Arc.Agent.Harness           ← 骨架：AISignalLog / AISignalEntry / AISignalLevel（分级 + KeySignal 标记
                               + 落盘 target/scratch/arc-logs/ + BuildLlmView(tokenBudget)）
  └─ Arc.Agent.Harness.Coding ← 判定/筛选：KeySignal 筛选规则、AIToolOutput 门面（exit + 摘要 + perf
                                摘要 + 日志路径引用）、perf 阈值（联动 performance-observability）
```

| 层 | 内容 | 允许 | 禁止 |
|----|------|------|------|
| `Arc.Agent.Harness`（基座） | `AISignalLevel` / `AISignalEntry` / `AISignalLog`（分级落盘 + `BuildLlmView` 骨架） | 类型契约、落盘面、字符代理约束实现 | 焊死 Coding 筛选规则（KeySignal 语义由 Coding 注入）；自造事件库 |
| `Arc.Agent.Harness.Coding` | KeySignal 筛选规则、`AIToolOutput` 门面、perf 摘要注入 | 领域筛选/判定规则 | 平行再造日志面 |

### AISignalLevel（日志分级）

```as
// 归属：Arc.Agent.Harness
public enum AISignalLevel {
    Debug,   // 详细调试（只落盘）
    Info,    // 常规信息（只落盘）
    Warn,    // 警告（落盘 + 摘要候选）
    Error    // 错误（落盘 + 强制进 LLM 摘要）
}
```

### AISignalEntry（单条日志）

```as
// 归属：Arc.Agent.Harness
public class AISignalEntry {
    public AISignalLevel Level;
    public string Timestamp;
    public string Source;      // 门 / 工具 / 阶段名（如 "D0-compile" / "quality.arc_test"）
    public string Message;
    public bool IsKeySignal;   // KeySignal 标记（Coding 规则注入，强制进摘要）
}
```

### AISignalLog（分级日志 + 筛选 + 落盘）

```as
// 归属：Arc.Agent.Harness
public class AISignalLog {
    public void Add(AISignalEntry entry);                    // 分级追加 + 落盘
    public void Flush();                                     // 落盘 target/scratch/arc-logs/
    public string BuildLlmView(int tokenBudget);             // 字符代理约束筛选折叠
    public List<AISignalEntry> Filter(AISignalLevel min);    // 分级筛选
    public List<AISignalEntry> KeySignals();                 // KeySignal 筛选
}
```

## 分级 / 筛选规则（哪些进 LLM vs 只落盘）

| 级别 | 落盘 | 进 LLM 上下文 |
|------|------|---------------|
| `Debug` | ✅ 全量落盘 | ❌（只落盘，可审计） |
| `Info` | ✅ 全量落盘 | ❌ |
| `Warn` | ✅ 全量落盘 | 摘要候选（按预算；`IsKeySignal` 时强制） |
| `Error` | ✅ 全量落盘 | ✅ 强制进摘要（含 `IsKeySignal`） |
| `KeySignal`（任意级别标记） | ✅ 全量落盘 | ✅ 强制进摘要（预算优先） |

- **全量在落盘**：筛选只影响「进 LLM 上下文」视图，**不丢审计面**——`target/scratch/arc-logs/` 全量可查。
- **KeySignal 语义由 Coding 注入**：perf 异常（`perf:anomaly`）、门判定失败、决策事件落地等标记 `IsKeySignal=true`；基座只持标记位，不定义「哪些是 KeySignal」。

## LLM 上下文预算（字符代理约束）

`BuildLlmView(int tokenBudget)`：

1. **token → 字符代理**：`charBudget = tokenBudget × 4`（1 token ≈ 4 字符的近似代理，诚实标注非精确；换算系数为设计参数，落地可调）。
2. **预算分配序**：`Error` > `KeySignal` > `Warn` > `Info/Debug`（预算不足时从低优先级开始裁剪）。
3. **折叠策略**：超预算条目折叠为「级别 + 计数 + 首末抽样」，不静默丢弃——被折叠的审计面仍在落盘全量中。
4. **边界**：字符代理约束**不替代** `AISession` 回合预算（038 §14）——前者约束上下文长度，后者约束回合数，正交。

## 落盘位置合规

- 日志根：`target/scratch/arc-logs/`（在 `.gitignore` 已含 `/target/` 下）——对齐工作区卫生规则，**禁写源码树**。
- 与既有落盘面并列：绿点快照 `arc-checkpoints/`（[DoD 绿点](definition-of-done.md)）、AIRfc 状态 `arcagent-state/`（[2.4 持久化](scenario-operation.md)）、perf 采集 `arc-logs/perf/`（[performance-observability](performance-observability.md) P1）。
- 日志不承载决策轨迹（§0 门闩 2）。

## AIToolOutput（工具输出门面 · Coding）

替代全量工具输出进 LLM 的结构化门面：

```as
// 归属：Arc.Agent.Harness.Coding
public class AIToolOutput {
    public int ExitCode;         // 工具退出码
    public string Summary;       // 语义摘要（如「build 成功 / 失败 N error」）
    public string? PerfSummary;  // AIPerfRun.PerfSummary（performance-observability 面）
    public string? LogPath;      // target/scratch/arc-logs/ 引用（全量可审计）

    public string ToContext(int tokenBudget);   // 经 BuildLlmView 字符代理约束折叠
}
```

- **字段**：`exit + 摘要 + perf 摘要 + 日志路径引用`，替代全量 stdout / stderr 进 LLM。
- **长度受预算约束**：`ToContext(tokenBudget)` 与 `AISignalLog.BuildLlmView` 同构（字符代理折叠），保证工具输出不撑爆上下文。
- **审计不丢**：全量输出仍落 `target/scratch/arc-logs/`，`LogPath` 提供引用——模型 / 人可查全量。

## 与 AITurnRunner / QualityTools 接线

| 面 | 接线 |
|----|------|
| `AITurnRunner`（[038](../../038-ai-host.md) 回合循环） | 工具调用返回经 `AIToolOutput` 结构化门面进入回合上下文（替代 raw 输出）；输出长度经 `BuildLlmView(tokenBudget)` 约束 |
| `QualityTools`（Coding） | `arc_build` / `arc_test` / `arc_inspect` / `arcgr_query` 结果经 `AIToolOutput` 折叠——退出码 + 语义摘要 + perf 摘要 + 日志路径引用；失败诊断仍走 `AIDoDFixFeedback`（结构化回喂）面（[DoD L2](definition-of-done.md)），`AIToolOutput` 是**工具输出进 LLM 的通用门面**，两者互补不重叠 |
| `AISignalLog` | QualityTools / 门执行内部经 `AISignalLog` 记录（Error / Warn 级 + KeySignal 标记），落盘 `arc-logs/` |
| perf 面 | perf 摘要注入 `AIToolOutput.PerfSummary`（[performance-observability](performance-observability.md) P2） |

## 演进 P1–P3（plan.md 登记 `PF-1–PF-3`）

| 阶段 | 内容 | 状态 |
|------|------|------|
| **P1 采集落盘**（`PF-1`） | `AISignalLog` 分级落盘 `target/scratch/arc-logs/`（Info / Warn / Error；Debug 级归 P2 筛选面）；与 `AIPerfMonitor` 采集（[performance-observability](performance-observability.md) P1）同批落盘 | **能力面已具备**：`AISignalLog.Add(level/source/category/line/keySignal)` + `WriteAsync(name, ct)` 落盘 `<project>/target/scratch/arc-logs/<tool>-<seq>.log`（seq 递增，对齐 AICheckpointStore 先例；禁写源码树）；`AIPerfMonitor` 经它落盘 perf 信号（wall/peak_memory/cpu_user/cpu_kernel/exit/timedout/anomaly） |
| **P2 筛选视图**（`PF-2`） | KeySignal 筛选规则（Coding 注入）+ `BuildLlmView(tokenBudget)` 字符代理 + `AIToolOutput` 门面（exit / 摘要 / perf 摘要 / 日志路径）；`AITurnRunner` 回合消费 | ⌛ 设计态（诚实缺口） |
| **P3 基线 + D9 门**（`PF-3`） | 日志 / 性能基线版本化（随绿点落盘）供 D9 性能门回归判定（阶段 E，联动 [performance-observability](performance-observability.md) P3） | ⌛ 设计态（诚实缺口） |

> P1/P2/P3 为设计权威演进名（用户面口径）；plan.md 排期标识 `PF-1–PF-3`（与既有 `P3 并行子代理` 里程碑名消歧）。

## 验收（五面推演判据，非测试全绿）

每阶段验收 = 所属场景**五面推演闭环**（[scenario-drive-acceptance](scenario-drive-acceptance.md)：A 输入 / B 真实代码路径 / C LLM 视角 / D 工具调用 / E 上下文）：

| 阶段 | 场景 | 五面推演判据 |
|------|------|-------------|
| P1 | B7（性能与可观测性门） | B 面真实：门 / 工具执行经 `AISignalLog` 分级落盘（非空想）；D 面返回可消费：日志路径引用可查 |
| P2 | B7 + 4.1（验收对照）+ B11（AI 能力边界） | C 面：模型上下文可见 `AIToolOutput`（exit + 摘要 + perf 摘要 + 日志路径），不再被全量输出撑爆（上下文预算面）；KeySignal 筛选后关键信号不丢 |
| P3 | A.7 + B7（D9 门） | 日志 / 性能基线版本化供 D9 门回归判定；「无观测不验收」断点消除才宣称交付（对齐 [B7 最小补件③](scenario-operation.md)） |

- **诚实边界**：任一阶段未过五面推演（如 `AISignalLog` 零调用、`AIToolOutput` 未接线）→ 该阶段不宣称交付；字符代理换算为近似值，禁止以代理精度冒充精确计量。

---

[返回 references 索引](index.md) · [返回 043(../../043-harness.md) · [性能观测与性能信号](performance-observability.md) · [可执行 DoD](definition-of-done.md) · [场景闭环推演验收协议](scenario-drive-acceptance.md)
