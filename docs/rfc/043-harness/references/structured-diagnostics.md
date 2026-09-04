# SR-2 结构化诊断（本体）设计

> 关联 [043 Coding Agent Harness 工程(../../043-harness.md) §3 · §6；承接 [可执行 DoD](definition-of-done.md)（D0 编译门信号源升级）与 [性能观测](performance-observability.md)（`AIDoDErrorItem` / 自适应折叠）。本文件是 **SR-2 本体**的设计权威：编译器侧 `--message-format json` 结构化诊断发射 + Harness 侧结构化消费与 D0 判定升级。
>
> **前置能力面（前哨）**：Harness 侧诚实启发式已具备——`QualityCli.ExtractErrorItems` / `ExtractWarningLines` 从 stderr 文本解析、`AIDoDErrorItem` 结构化载体、`FoldErrors` / `FoldWarnings` 自适应折叠、D0 门绿时告警作 Warn 咨询信号（绿灯不造假）。本文件规划的是**编译器本体发射**，让 Harness 从「解析文本启发式」升级为「消费结构化线协议」。
>
> **性质**：**目标能力面（诚实缺口）**——编译器 `--message-format json` 发射 + `QualityCli.ParseDiagLines` 消费 + D0 结构化判定均未实现，排期遵循「单目标 Sprint · 不自动开干 · 单 M · 子集边界」。**未落地前不得据此宣称任何能力完成**；前哨的启发式路径保持为唯一已落地面。

## §0 宣称门闩

| # | 门闩 |
|---|------|
| 1 | **设计态**：本文件全部为规划，`--message-format json` 编译器发射 + `QualityCli.ParseDiagLines` 消费 + D0 结构化判定 **均未实现**；禁止据此宣称「结构化诊断已落地」 |
| 2 | **零冻结面改动**：SR-2 本体只在 `arc` crate（pipeline.rs / main.rs）与 Harness 消费侧落地，**不得**改动语言核心（`parse` / `typeck` 错误枚举补 span、`mir` NLL 内部结构）——那属冻结面 / SR-2b 后续项 |
| 3 | **诚实回退**：结构化数据不可用（旧编译器 / 崩溃前无发射 / 解析失败）时，D0 判定**回退现有启发式**（exit + stderr 文本），不得因解析失败制造假绿或假红 |
| 4 | **双通道并存**：`--message-format json` 下人类渲染**不抑制**，NDJSON 行追加到 stderr；消费者只消费 `{` 前缀行，人类行天然隔离，信息零丢失 |

## 目标

- 编译器：`arc build` / `arc check` 增加 `--message-format json`，将各阶段诊断（错误 / 警告）以**单行 JSON（NDJSON）** 发射到 stderr，字段含 `severity / code / phase / message / file / line / col / suggestion`。
- Harness：`QualityCli` 以结构化解析替代 stderr 文本启发式，`AIDoDErrorItem` 直接从 JSON 记录填充；**D0 判定升级为「结构化 0 error + 0 warning」**（闭合 [definition-of-done](definition-of-done.md) 已知限制 K7）。
- 对齐行业最佳实践：线协议对标 Rust `rustc --message-format json`（NDJSON · 每诊断一行 · 机器可消费），Consumer 端以轻解析 + 原文旁路给模型，避免在 Arc 侧引入重型 JSON 库（去过度设计 B5）。

## 非目标（范围边界 · 单 M 子集）

| 项 | 说明 | 去向 |
|----|------|------|
| `TypeError` / `ParseError` 枚举补 span 字段 | 触碰语言核心冻结面；本设计以「诚实 null」上报位置缺省，不借机改冻结面 | **SR-2b**（独立排期，需评估是否走 RFC 036 流程） |
| 子图级增量验证（`--incremental-report` 粒度升级） | 涉及编译器增量编译机制，独立于诊断线协议 | **SR-2c**（K5，独立排期） |
| `suggestion` 全量供给 | 仅 NLL（`E_*`，`from_invalidation` 自带建议）现成；其余阶段 v1 为空，协议字段预留 | 后续阶段 |

## 1 现状调查（发射点全景）

编译器错误统一经 `Err(String)` 传播到 `main.rs` 的单一出口 `eprintln!("error: {e}")` → `exit(1)`（[main.rs(../../../../crates/arc/src/main.rs)）；警告与解析错误在 pipeline 内直接写 stderr。各阶段携带的结构化信息如下：

| 阶段 | 发射点 | 人类格式 | 有码 | 有 span | 备注 |
|------|--------|---------|------|---------|------|
| Parse | `emit_parse_error`（pipeline.rs） | codespan `error: <msg>` + 彩色标签 | ❌ | ✅ | 有 file_id + 字节偏移，可算 line/col |
| Desugar | `prepare_compilation` join → `Err(String)` | `error: <joined>` | ❌ | ❌ | 纯文本 |
| HIR | `format!("hir error: {e}")` | `error: hir error: <e>` | ❌ | ❌ | 纯文本 |
| Typeck | `TypeError::to_string()` join → `Err(String)` | `error: <msg>` | 仅 `Macro` 变体 | ❌ | 枚举大多无 span/码 |
| Warning | `TypeWarning.render()`（pipeline.rs） | `warning[arc-cycle-001]: <msg>` | ✅ | ✅ | 类声明 span |
| 静态初始化 | `render_static_init_diagnostics` | `warning[arc-sinit-001/002]: <msg>` | ✅ | ❌ | `StaticInitDiagnostic` |
| NLL | `mir::dataflow` → pipeline.rs | 错误 → 编译失败 | ✅ | ❌ | `E_BORROW_CONFLICT` / `E_ITERATOR_INVALIDATION`；fn_name + local id |
| Codegen | `codegen error: {e}` | `error: codegen error: <e>` | ❌ | ❌ | 纯文本 |
| Link | `link error: {e}` | `error: link error: <e>` | ❌ | ❌ | 纯文本 |
| ICE | `phase_guard` | `internal compiler error during <phase>: <msg>` | ❌ | ❌ | 防御兜底 |

> 结论：**协议层已齐备的只有 warning 面（有码 + 部分有 span）**；错误面大多只有消息文本。SR-2 本体的策略是「**数据在哪就在哪富化，数据缺则诚实 null**」，不借机改冻结面。

## 2 线协议（NDJSON，对齐 Rust `--message-format json`）

`arc build --message-format json`（`arc check` 同）在 stderr 追加**每诊断一行**的 JSON 对象（追加到人类渲染之后，人类输出不抑制）：

```jsonc
{"severity":"error","code":null,"phase":"parse","message":"unexpected token: expected ;, found }","file":"src/Program.as","line":12,"col":5,"suggestion":null}
{"severity":"warning","code":"arc-cycle-001","phase":"typeck","message":"类 A 与 B 存在声明级字段环","file":"src/Program.as","line":3,"col":1,"suggestion":null}
{"severity":"error","code":null,"phase":"typeck","message":"type mismatch: expected string, found int","file":null,"line":null,"col":null,"suggestion":null}
```

字段约定（与 `AIDoDErrorItem` 一一对应，消费侧零映射成本）：

| 字段 | 类型 | 语义 | v1 供给 |
|------|------|------|---------|
| `severity` | `"error"` \| `"warning"` | 严重级 | 全阶段 |
| `code` | string \| null | 诊断码（`arc-cycle-001` / `arc-macro-XXX` / `E_*` / `arc-sinit-001`…） | warning 面 + NLL + Macro |
| `phase` | string | `parse` \| `desugar` \| `hir` \| `typeck` \| `mir` \| `codegen` \| `link` \| `internal` | 全阶段 |
| `message` | string | 人类可读消息（P3 措辞约定不变） | 全阶段 |
| `file` | string \| null | 源码路径（display 形式） | 有 span 处 |
| `line` / `col` | int \| null | 1-based 行列 | 有 span 处 |
| `suggestion` | string \| null | 修复建议 | NLL（`from_invalidation` 现成） |

转义约定（消费侧轻解析的前提，发射时强制）：

- `"` → `\"`，`\` → `\\`，换行 → `\n`（消息内部禁止出现裸引号/换行）。
- 位置缺省一律 `null`（**诚实**：不伪造 0/空串，模型可据此判断「无定位」）。
- 实现上 `arc` crate 内自持极简 JSON 转义 + 序列化（`serde_json` 可复用，`crates/arc` 已有依赖），不新增第三方重量库。

## 3 发射点接线（编译器侧 · 零冻结面）

### 3.1 富化层（数据已存在处，逐点加 JSON 分支）

| 发射点 | 改动 | 产出 |
|--------|------|------|
| `emit_parse_error`（pipeline.rs） | 由 `Span` + 源码字节偏移算 `line/col`（`file_id` → 路径经 `file_registry`），追加 JSON 行 | 富化 `parse` 错误 |
| typeck warning 打印点（pipeline.rs `TypeWarning` 循环） | 每个 `TypeWarning`（有 `code` + 类 span）追加 JSON 行 | 富化 `warning` |
| `render_static_init_diagnostics` | 每个 `StaticInitDiagnostic`（有 `code()` / `message()`）追加 JSON 行 | 富化 `warning` |
| NLL 诊断打印点（pipeline.rs `run_nll_check_module` 非空分支） | `NllDiagnostic`（有 `code` + `fn_name`）追加 JSON 行；`fn_name` 拼入 `message`，位置诚实 null | 富化 `mir` 错误 |

### 3.2 兜底层（统一错误出口 `main.rs:742`）

`--message-format json` 下，`eprintln!("error: {e}")` 改为：按消息前缀推断 `phase` 后追加 JSON 行（`parse error:` → `parse`，`hir error:` → `hir`，`codegen error:` → `codegen`，`link error:` → `link`，`internal compiler error during <p>:` → `internal` + phase 提取，其余 → `typeck`/`desugar` 按来源标注）。**去重**：`parse error:` 前缀的记录已在富化层发射，兜底层跳过，避免重复行。

> 该层覆盖 desugar / hir / typeck / codegen / link / ICE 的全部错误面，保证「任一失败必有结构化记录」；位置/码诚实 null，交由后续 SR-2b 富化。

### 3.3 exit code 语义不变

`--message-format json` 只改变**诊断输出形态**，不改变 exit code 判定（`error` → exit≠0，纯 `warning` → exit 0）。Harness 的「绿灯不造假」契约继续由 D0 判定升级承载（见 §4.2）。

## 4 消费升级（Harness 侧）

### 4.1 `QualityCli.ParseDiagLines`（新增）

从 `ProcessRunResult.StandardError` 提取 `{` 前缀行 → 极简 NDJSON 字段提取（`IndexOf("\"key\":")` + 定界读取，处理 `\"` / `\\` / `\n` 转义）→ 填充 `AIDoDErrorItem`（`File/Line/Col/Code/Message/Suggestion`）并携带 `severity`/`phase`。无 `{` 行（旧编译器 / 崩溃前无发射）→ 空列表，触发回退。

> **架构注记**：该轻解析刻意**不引入 JSON 库**——字段集合固定、发射端保证转义，Arc 侧以既有 `string` 操作即可完成；原文行保留旁路给模型（LLM 深度消费不受折损）。若未来字段面扩大，再评估 `std/Data` 或新子库提供完整 JSON 解析（非本子集）。

### 4.2 D0 判定升级（K7 → K9）

`CodingDoDGateEvaluator.EvaluateD0Async`：

- 有结构化记录时：`error` 记录数 > 0 → **Fail**（`ErrorItems` 从 error 记录填充，`Detail` 折叠摘要）；`error` = 0 且 `warning` = 0 → **Passed**（判定升级为「结构化 0 诊断」，闭合 K7）；`error` = 0 但 `warning` > 0 → **Passed + Warn 信号**（同前哨契约，绿灯不造假）。
- 无结构化记录时：**回退现有启发式**（`ExtractErrorItems` / `ExtractWarningLines` + exit 码），诚实不造假。
- `QualityCli.IsGreen` 语义：在结构化路径下由「exit=0」升级为「0 error 记录」，exit 码保留为旁证（belt-and-braces）。

### 4.3 `AIDoDErrorItem` 消费面不变

`ErrorItems` 结构字段与线协议一一对应，`AIDoDFixFeedback.Describe()` 结构化渲染路径**无需改动**，仅数据源从「启发式解析」换成「JSON 记录」。

## 5 验收

新增 e2e `arc_ai_dod_d0_structured`（对齐 `arc_ai_perf_observability_e2e` 的 fixture 注入模式）：

| 场景 | 断言 |
|------|------|
| 绿 + 0 诊断 | `arc build --message-format json`（正常 fixture）→ D0 Passed，error 记录 = 0，无 Warn 信号 |
| 绿 + warning | 含环 fixture（`warning[arc-cycle-001]`）→ Passed + `severity=warning` 记录被识别 + Warn 信号 |
| 红 + error | 类型错误 fixture → Fail + `ErrorItems` 从 JSON 填充（`severity=error` 记录、消息可断言） |
| 线协议 | 单行 JSON 可被 `ParseDiagLines` 正确还原 `code/file/line/col/message`（含转义） |

回归：`arc_ai_dod_fix_loop_e2e`（回喂渲染不破坏）、`arc_ai_perf_observability_e2e`（前哨路径兼容）。

## 6 落地顺序（排期开工后）

1. `arc` crate：`--message-format json` 参数 + `diag` 模块（结构 + 转义 + 序列化）+ 富化层逐点接线 + 兜底层去重（§3）。
2. `arc check --message-format json` 对齐（若 `check` 复用什么路径则自然覆盖）。
3. Harness：`QualityCli.ParseDiagLines` + D0 判定升级 + 回退逻辑（§4）。
4. e2e `arc_ai_dod_d0_structured` + 回归（§5）。
5. 关闭 plan.md SR-2 行：把 definition-of-done D0 行「现状」注记切到「目标」态。
