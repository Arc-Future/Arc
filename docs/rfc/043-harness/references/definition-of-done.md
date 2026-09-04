# 可执行 DoD 与验证闭环

> **注（2026-08-29）**：原 `crates/arc-integration` 已退场（a2627a0f）。本文所引
> `cargo test -p arc-integration ...` 验证命令不再可用；现行验证矩阵为
> `cargo test --workspace`（运行时面 `cargo test -p arc-tests --features full-rt`），
> 详见仓库根 `CHANGELOG.md`。

> 关联 [043(../../043-harness.md) §3 · §5 · §6。本子项定义 D0–D7 各门的**可执行判定**（Coding 领域信号）、信号归属层、L2/L3 边界、绿点快照与回滚。需求本尊见 [AIRfc](airfc.md)；纠偏见 [纠偏协议](anchor-correction-protocol.md)；宣称门闩见 [llm-gates](llm-gates.md)。

## §0 宣称门闩

| # | 门闩 |
|---|------|
| 1 | **`AIPlan` / 任务 `Completed` ⇔ D0–D7 全勾 + 涉真实外部系统经 D8 真实接入冒烟门**；缺一禁止标 `Completed` |
| 2 | **未接线的门必须 `Pending`**；禁止标 `Passed` 假绿（D2/D4 等在接线完成前不得假绿；D1/D6 已接线） |
| 3 | **门骨架在基座，判定信号在 Coding**；不得用基座空壳冒充领域全绿 |

> **M8 语义**：`AIPlan` 满额步骤只进 `Verifying`（待判定态）；`Completed` 唯一写入路径 = DoD D0–D7 全勾经受控 API（`AIPlanGate.CompleteByDoD` / `AIHarnessSession.CompletePlanAfterDoDAsync`）；未接线门保持 `Pending`，`AllPassed` 禁把 `Pending` 当通过。

## 信号归属层（每门适用）

| 层 | 职责 |
|----|------|
| `Arc.Agent.Harness`（基座） | DoD **门骨架**、状态流转、绿点/回滚协议、与 AIRfc Revision 对齐 |
| `Arc.Agent.Harness.Coding` | **判定信号**（`quality.*`：`arc_build` / `arc_test` / `arc_check` / `arcgr_query` 等）与 D0–D7 Coding 判定实现 |

每门下表「信号归属」标明：骨架 vs Coding。骨架就绪 ≠ 判定已接线。

## D0 编译门（全自动）

| 面 | 契约 |
|----|------|
| 信号归属 | 骨架：门状态；**Coding**：`arc build` / `quality.arc_build` |
| 信号源 | **目标**：`--message-format json` 结构化诊断（NDJSON 线协议，设计见 [structured-diagnostics](structured-diagnostics.md)）；**诚实缺口**：该发射未落地，当前实际为退出码 + stderr 文本（勿依赖结构化线协议） |
| 判定 | **目标**：结构化 0 诊断——error 记录 > 0 → Fail，error = 0 且 warning = 0 → Passed（设计见 [structured-diagnostics](structured-diagnostics.md)）；**诚实缺口**：`QualityCli.IsGreen` 仅查 `exit=0`，「0 warning」未落地（见已知限制 K7） |
| 回喂 | **目标**：消费 NDJSON 记录 → `AIDoDErrorItem`（`file:line:col + code + message + suggestion`）结构化信号（设计见 [structured-diagnostics](structured-diagnostics.md)）；**诚实缺口**：当前为诊断文本（`exit/stdout/stderr` 折叠 + 诚实启发式）喂回模型 |
| 效率 | **目标**：借助代码图做**增量验证**——只重编受影响子图；**诚实缺口**：当前为全量 build（`--incremental-report` 粒度为文件级，见已知限制 K5） |
| 性能信号增强 | 增强信号（不新开门）：`AIDoDGateResult.PerfSignals` 挂 D0 门结果——Stopwatch 墙钟 + `rt_proc_get_stats` 内存/CPU + 超时熔断 + 退出分类（设计见 [performance-observability](performance-observability.md)）；D9 性能门（`arc bench` 基线回归阈值）为阶段 E 增强门，不参与 `Completed` 判定链 |

失败动作：喂回模型迭代（≤3 轮，见 L2 边界）。不过 D0 不进入 D1/D2/D3。

## D1 语义完整性门（全自动，Arc 独有强项）

| 面 | 契约 |
|----|------|
| 信号归属 | 骨架：门状态；**Coding**：`.arcgr` / `quality.arc_inspect` / `quality.arcgr_query` |

**已接线（H-3）**：`CodingDoDGateEvaluator.EvaluateD1Async` 跑 `arc inspect <entry> --format json`（源码模式）取 symbols/edges/entry_points/reachable/unreachable，入口符号跑 `arc explain <arcgr> <sym> --format json` 取 callers/callees/reference_count/is_reachable。通过谓词（最小可证伪、**不覆盖语义正确性**、不造假）：inspect exit 0 + 有 symbol 表 + `edges` 键可导出 + 无「引用断裂」（每条边 caller/callee 端点均在符号表）+ 无「不可达入口」（入口符号 ∈ reachable）+ 入口 explain `is_reachable` 一致。unreachable 符号属正常输入面（私有未用方法 / 库导出面未被本 TU 消费等），不判红；仅在「引用断裂 / 不可达入口」这类机器可判信号上判红。

| 检查 | 契约 |
|------|------|
| 引用完整性 | 改符号/删 API → `.arcgr` 引用图全仓库引用点已同步更新 |
| 契约影响 | 改 public API → 下游使用点已验证（impact 分析） |
| 可达性 | 新增/删除入口 → reachability 分析确认无孤儿代码/死入口 |
| 语义零分叉 | 信号源为 `.arcgr` 产物与编译期语义（[034(../../034-ai-toolchain-arcgr.md)），非 grep 猜测 |

## D2 契约硬规则门（全自动）

| 面 | 契约 |
|----|------|
| 信号归属 | 骨架：门状态；**Coding**：契约扫描 / `quality.arc_check` |
| 未接线 | **必须 `Pending`**；禁止空扫标 `Passed` |

**已接线（H-3）**：`CodingDoDGateEvaluator.EvaluateD2Async` 跑 `D2ContractScanner`（源码级真实扫描，收集目标 `.as` 文件集，文件直用/目录递归跳过 obj/bin/target/.git）。通过谓词（最小可证伪、不造假）：空文件集 → **Pending**（数据不足，非 Passed）；逐文件行级扫描，逐项给出 通过/失败 + 命中样例（file:line），任一命中 → **Failed**。契约可查项由机器判定（**H-3 接线仅 3 个机器可查项**：async `Async` 后缀 / Allman 大括号 / 类型·方法 PascalCase，其余诚实列 SkippedRules，不冒充全绿），**硬规则不过必改、不协商**（对标 [002 语法表面与编码标准(../../002-surface-contract.md)）：

| 契约 | 可查项（H-3 接线） |
|------|--------|
| 异步规范 | 返回 `Task` / `Task<T>` 的方法声明必须以 `Async` 结尾（入口 `Main` 例外）；`CancellationToken` 面/同步 I/O 副本未接 |
| 控制流 | `if`/`else`/`while`/`for`/`foreach`/`switch` 一律 `{}` 括起；左花括号独立成行（Allman；K&R 同行花括号与省略大括号单语句判红） |
| 命名规范 | 类型声明（class/interface/struct/enum）与方法声明 PascalCase（参数/私有字段面未接） |
| `this.` 成员前缀 | 源码级近似无法承载符号判定 → **未接线项**（诚实列在 SkippedRules，不冒充全绿） |
| `[Builtin]` stub | 禁自动属性——需 ABI 感知 → **未接线项**（同上） |
| 单一惯用法 | 禁双轨写法/原始样板式——语义判定 → **未接线项**（同上） |

## D3 行为验证门（自动 + 边界升级）

| 面 | 契约 |
|----|------|
| 信号归属 | 骨架：门状态；**Coding**：`arc test` / `quality.arc_test` + 产物断言 |
| 信号源 | `cargo test -p arc-integration --test <e2e>`（Arc 开发者核心验证面）+ 产物运行断言；QIF `arc test`（[032(../../032-qif.md)） |
| 判定 | **用例级明细**：解析 `arc test --logger json` 的 `summary`/`results`（`D3TestReport`）——`passed > 0 且 failed == 0` 才绿（**断言绿 ≠ 退出码绿**）；结构化 Acceptance 条目 `TestName` 须在结果中真实 `passed`（验收对照）；**防降级**：会话内基线（已见最大用例数）——用例数骤降（如 N→0）标疑判红（`test count reduced`），改测 = [AIRfc 纠偏](anchor-correction-protocol.md)；断言**独立于实现**（见 [testing-first](testing-first.md)） |
| 分级解锁 | D0/D1/D2 不过不进入本门；按影响面选测试等级：改 `std/**` → 相关 crate 集成 + 受影响 e2e；改 `examples/**` → 该 example 的 build + e2e |
| 效率 | 借助依赖图只跑受影响测试（增量测试选择） |
| 防降级 | 已批准验收测零擅自削弱；改测 = [AIRfc 纠偏](anchor-correction-protocol.md) |
| 性能信号增强 | 增强信号（不新开门）：`AIDoDGateResult.PerfSignals` / `AIDoDFixFeedback.PerfSignals` 挂 D3 门结果与失败回喂——测试执行墙钟 / 内存 / CPU / 超时熔断（设计见 [performance-observability](performance-observability.md)）；D9 性能门（基准双跑对比 + 回归阈值，基线版本化随绿点落盘）为阶段 E 增强门，不参与 `Completed` 判定链 |

### L2 自动化边界

**自动判定（无需人）**：
- 编译错误、测试失败 → 结构化断言 diff（期望 vs 实际）喂回迭代，≤3 轮。
- 迭代收敛 → 进入 D4。

**升级人（L2 内边界，升级非卡死）**：

| 场景 | 判定 | 动作 |
|------|------|------|
| 验证信号不可信 | 测试与实现同源（自证风险） | 验收测试先行锁定 / 断言来自 AIRfc Acceptance，见 [testing-first](testing-first.md) |
| 语义意图冲突 | 测试全绿但行为不符 AIRfc 隐含意图（模型不能自证意图） | 升级人确认方向 |
| 验证环境不可用 | 缺 `ARC_DEEPSEEK_API_KEY`、平台依赖缺失 | 升级人，不重试烧钱 |
| 迭代超限 | 3 轮内不收敛 | 自动回滚最近绿点 + 升级人 |

## D4 diff 覆盖门（全自动）

| 面 | 契约 |
|----|------|
| 信号归属 | 骨架：门状态；**Coding**：diff ↔ `AIPlan` 步骤对齐 |
| 未接线 | **必须 `Pending`**；禁止无覆盖证据标 `Passed` |

**已接线（H-3）**：`CodingDoDGateEvaluator.EvaluateD4Async` 比对 `AIPlan.Steps` 文件声明与工作区改动集（`D4DiffCoverage`）。通过谓词（最小可证伪、不造假）：**无 AIPlan 步骤 → `Pending`**（数据不足，非 Passed）；改动集主信号 = `git status --porcelain -- .`（工作目录 = 目标项目，pathspec 限定子树，含已跟踪修改 + 未跟踪新文件）；git 不可用（spawn 失败/非 git 仓库）→ **兜底最小判定**：目标项目 `.as` 文件清单 ∩ 计划文件声明（交集视为「对应改动存在」）；判定：**计划覆盖**——每个声明了文件名的步骤（`Files` 逗号分隔，取 basename）至少有一处对应改动，无任何改动 → 显式 no-diff（视为覆盖满足）；**越界检测**——每个改动文件须被至少一个步骤声明，声明为空且有改动 → 全部越界；覆盖不足 / 越界 → **Failed**（带步骤/文件命中）。

| 检查 | 契约 |
|------|------|
| 计划覆盖 | 每个 AIPlan step 有对应改动（代码图 diff 影响面 ↔ 计划步骤对齐） |
| 越界检测 | 改动超出 AIRfc / 计划边界 = **过度设计信号** → 升级人确认（收敛检查落点，见 [design-review](design-review.md)） |

## D5 自审收敛门（自动 + 人抽查）

| 面 | 契约 |
|----|------|
| 信号归属 | 骨架：自审报告槽位；**Coding**：对照 AIRfc Acceptance 附证明 + 机器校验 |
| 自审报告 | 模型对照当前 AIRfc Revision 输出：每条 acceptance 附**可执行证明**（哪个测试/哪个文件）。**机器校验**：`D5SelfReview.SetProof` 委托 `D5ProofVerifier`（Coding）校验证明**引用存在性**——文件存在（相对项目根 / 绝对路径）或测试名在 `arc test --list-tests` 输出中可解析；无机器校验（Unchecked）/ 引用不存在（Invalid）**标红而非 Passed**，`AllProven` 要求全部 Valid；深度校验（证明「真实运行通过」）为增强面 |
| 无证明项 | 补验证或升级人 |
| 抽查面 | 人抽查自审报告的真实性（不重读全部 diff） |

## D6 反模式门（全自动）

| 面 | 契约 |
|----|------|
| 信号归属 | 骨架：门状态；**Coding**：反模式可查项扫描 |
| 未接线 | **必须 `Pending`**；禁止空清单标 `Passed` |

**已接线（P2）**：`CodingDoDGateEvaluator.EvaluateD6Async` 做源码级确定性扫描（`D6AntiPatternScan`），判红项最小集（可机器查、不造假）：占位壳（`NotImplemented` / `NotImplementedException` / `todo!()`）与 `TODO`/`FIXME` 注释标记（`//` / `/*` 注释内）。通过谓词：扫描文件数 > 0 且零命中 → Passed；有命中 → Failed（命中清单 `path:line: marker` 回喂修复）；无 `.as` 源文件 → Pending（数据不足，禁空扫 Passed）。unreachable 死符号不判红——unreachable 属正常输入面（私有未用方法 / 库导出面未被本 TU 消费），「定义于项目且零引用且非导出」的可疑死符号需精确引用图（D1 `.arcgr` 范畴），D6 保持源码级确定性判定、不武断判红。

arc-core 反模式清单的可查项全过；不可自动判定的设计层面反模式标记给人（见 [collaboration-checkpoints](collaboration-checkpoints.md)）。

## D7 人验收门（人）

| 面 | 契约 |
|----|------|
| 信号归属 | 骨架：确认点协议；确认结果写入 Agent 会话事件（决策轨迹） |
| 判定 | 协作确认点经人确认；未确认不入 `Completed`。**要求**：调用方不可程序化假确认（真实人确认不可用布尔替代）。**诚实缺口（D-08）**：`CompletePlanAfterDoDAsync(d5Confirmed, d7Confirmed)` 允许调用方以布尔参数把人类门覆盖为 `Passed`（`ApplyHumanGates`）；本门通过不得依赖该布尔覆盖 |

确认点清单见 [collaboration-checkpoints](collaboration-checkpoints.md)。

## D8 真实接入冒烟门（条件前置门）

> **性质**：D8 是**条件前置门**——仅当能力涉及**真实外部系统**（LLM Provider / HTTPS / 外部 CLI / 真实项目 build+test）时才适用；纯离线 / 纯本地能力不受本门约束。本门是 `Completed` 的前置条件之一，不是 D0–D7 的顺延编号。来源见 [harness-llm-lessons](harness-llm-lessons.md)（B3 机制 · P0-1）。

| 面 | 契约 |
|----|------|
| 适用条件 | 能力依赖真实外部系统（LLM Provider / HTTPS / 外部 CLI / 真实项目 build+test） |
| 前置 | `Completed` 前置 = **一次真实连通冒烟**：真实 key / endpoint / 真实项目跑一遍，留 trace（命令 + 输出 / 日志路径） |
| 证据 | 真实连通 trace；真实 key 经 `$env:KEY` 注入、**禁落盘**；冒烟结果落 `target/scratch/` 或 `$env:TEMP` |
| 反例 | e2e fixture（mock / 本地断言请求体）是**回归证据**，**不替代**真实连通冒烟 |
| 未冒烟 | 涉及真实外部系统的能力**不得**标 `Completed`；可标 `Pending` 或如实挂账 |

**失败动作**：真实冒烟失败 → 修复后重跑；不得以 e2e fixture 绿为由绕过本门。

## L3：机器筛平凡项 vs 人审

L3 不替代 D7，而是**降低噪音、抬高沟通价值**：

| 面 | 契约 |
|----|------|
| 机器筛 | 可查硬规则、可复现失败、平凡越界提示 → 自动处理或结构化上报，不打断人 |
| 人审 | 高沟通价值点（API/ABI、需求冲突、设计合理性、意图确认）→ 见 [collaboration-checkpoints](collaboration-checkpoints.md) |
| 纪律 | 机器筛过 ≠ 领域全绿；未接线门仍须 `Pending`；人审未确认不得 `Completed` |

## 绿点快照与回滚

| 面 | 契约 |
|----|------|
| 绿点 | D0–D3 全绿即打 diff 快照 + 记会话事件（对齐当前 AIRfc Revision） |
| 安全网 | 全自动的安全不是"别出错"，是"出错能回到最后的好状态" |
| 回滚 | 迭代超限/异常 → 回滚最近绿点，不丢历史 |
| 可追溯 | 绿点进决策轨迹 → 可 /resume、可复盘；与 [纠偏协议](anchor-correction-protocol.md) 事件语义并列 |

**绿点实现面**：`AIHarnessSession.CheckpointGreenAsync` 捕获**真实绿点快照**——工作区关键状态（`git rev-parse HEAD` + `git stash list`，best-effort）+ 文件清单快照（递归收集项目根下常规文件，跳过 `.git`/`target`/`obj`/`bin`/`.arcagent` 等产物与元数据目录；小文件 ≤64KB 存全文 + SHA256 供无 git 回滚，大文件仅登记存在）——落盘 `<project>/target/scratch/arc-checkpoints/latest.json`，随后发 `checkpoint:green` 事件（事件 Detail 标注快照路径或 `snapshot:none`）。`CheckpointRollbackAsync` 按**最近绿点**快照**真实回滚**（非只发事件）：恢复清单内差异文件内容、删除快照后新建文件（清单截断时不删新建——删除语义不完整时禁用）；大文件缺失时 git 环境经 `git checkout --` 恢复。无快照 → 返回失败并升级人（`checkpoint:rollback` 事件仍记轨迹，Detail 折叠恢复/删除/跳过计数）。项目根不可解析 → 快照不捕获（不建目录、不污染）。

## 与 AIRfc Revision / 纠偏

| 面 | 契约 |
|----|------|
| 事实源 | 执行环对照**当前 AIRfc Revision**（Intention / Design / Acceptance + `AIPlan` 引用） |
| 升版 | 方向变更 → Revision+1 → DoD 门重对齐；旧绿点只读审计 |
| 回滚（AIRfc） | 绿点回滚经 `AIRfcRuntime.RestoreRevision` 恢复旧 Revision（Superseded → Active，不递增）；**必须持 `AILeaseKind.RfcSpec` 租约**（后到拒绝，不绕过冲突织物）——airfc §4.2 回滚例外 |
| 修复 | 实现失败不升 Spec 版；在本门迭代 ≤3 轮，超限回滚 |
| 交叉 | 分流细则见 [纠偏协议](anchor-correction-protocol.md)；宣称见 [llm-gates](llm-gates.md) |

## 已知限制（实测断点 · 编译器 / arcgr 专项输入）

> 下列限制是文档宣称与当前工具能力之间的实际差距，作为编译器 / arcgr 专项输入登记；未落地前不宣称相应门全绿。「已收敛」项不再构成开放限制，仅保留判别性描述。

| # | 限制 | 影响门 | 处理 |
|---|------|--------|------|
| K1 | `.arcgr` 符号收集器**不收录顶层自由函数（含 `Main`）**：纯 Main 入口 `arc inspect` → symbols=0 / entry_points=0，D1 必红 | D1 | 已收敛：`crates/arc/src/arcgr.rs` 补顶层函数符号收集 + `Main` 入口点 |
| K2 | `arc inspect` 为**单文件 MVP**，跨文件引用直接崩溃（未解析兄弟文件） | D1 | 已收敛：`collect_arcgr_file` 改以**项目文件集合**过滤（符号 / 入口 / 边 / 虚分派按「文件 ∈ 项目集合」收集）；自由函数符号改**裸名**对齐 `typed_fns.name`（原 FQN 前缀致跨命名空间边查无符号静默丢弃） |
| K3 | 方法→方法调用 `edges=0`（实例/类方法引用图无可查数据） | D1 | 已收敛：`resolve_method_callee` 改「接收者类型 → 方法符号」解析（局部类型表 + 接收者形态覆盖，拼 `"Class.method"` 查符号表）；类方法体经 `typed_fn_symbol_name` 剥离 `Class::` link 名前缀被遍历 |
| K4 | `arc test <含 Main 的 app 项目>` 必然失败（QIF 合成 `__QifTestHost.Main` 与 app `Main` 冲突） | D3 | 已收敛：合成入口前 `strip_entry_main` 剔除用户顶层 `Main` 自由函数，测试入口由 `__QifTestHost::Main` 接管；类方法 `Main` 不受影响 |
| K5 | `--incremental-report` 粒度=**文件级**（rebuilt_files 计数），非文档「受影响子图」 | D0 效率 | 子图级增量验证（设计见 [structured-diagnostics](structured-diagnostics.md) 非目标） |
| K6 | D4 三处失效：① gitignored 盲区（产物在 `target/` 下 `git status` 看不到 → changed=0 假红）；② 兜底用全路径 vs basename 精确相等 → 恒假；③ 文档「无改动 → no-diff 满足」与代码不一致 | D4 | 已收敛：`D4DiffCoverage` 三修——① git 信号感知 gitignored 工作区（退回文件系统清单兜底）；② 兜底交集按 basename 对齐；③ no-diff 谓词与文档一致 |
| K7 | D0「0 warning」无实现：`QualityCli.IsGreen` 仅查 `exit=0`，std 库自带 cycle 警告 + clang 链接警告不判红 | D0 | 结构化 0 诊断判定（设计见 [structured-diagnostics](structured-diagnostics.md)） |
| K8 | `arc build <arc.toml>` 把 toml 当源码解析失败，文档未写「传项目目录」 | D0 | 文档补参数姿势；或编译器接受 toml 路径 |

## DoD 清单（可勾选）

```
□ D0 编译零错误（arc build 全绿；Coding 信号；**warning 面未判定**，见已知限制 K7；**性能信号增强 + D9 性能门（阶段 E 增强门，不参与 `Completed` 判定链）**，见 [performance-observability](performance-observability.md)）
□ D1 引用/契约/可达性全绿（arcgr；未接线 → Pending，禁假绿）
□ D2 契约硬规则全过（未接线 → Pending，禁假绿）
□ D3 验收测试全绿（`--logger json` 用例级明细：passed>0 且 0 失败；防降级：用例数骤降判红、结构化 Acceptance TestName 对照；断言独立于实现；**性能信号增强 + D9 性能门（阶段 E 增强门，不参与 `Completed` 判定链）**，见 [performance-observability](performance-observability.md)）
□ D4 diff 对 AIPlan 覆盖、无越界（未接线 → Pending，禁假绿）
□ D5 自审报告：每条 AIRfc acceptance 有**机器校验**的可执行证明（引用真实测试/文件，`--list-tests` 可解析；无校验/无效标红）
□ D6 反模式可查项全过（源码级扫描已接线；无源文件 → Pending，禁假绿）
□ D7 协作确认点经人确认
□ D8 真实接入冒烟门（仅涉真实外部系统的能力；真实 key/endpoint/真实项目跑一遍留 trace；e2e fixture 不替代冒烟）
```

**硬句**：`AIPlan` / 任务 `Completed` ⇔ 上表 D0–D7 **全勾** + 涉真实外部系统的能力经 **D8 真实接入冒烟门**；缺一禁止 `Completed`（替代"声称完成"）。

> **D9 性能门（阶段 E 增强门）**：性能观测与性能信号能力见 [performance-observability](performance-observability.md)。增强信号不新开门（`AIDoDGateResult.PerfSignals` / `AIDoDFixFeedback.PerfSignals` / `perf:anomaly` 事件，不改 `Pending ≠ Passed` 语义）；`AIDoDGateKind.D9Perf`（`arc build` 基线 diff 回归阈值）由 `AIPerfBaseline`/`AIPerfBaselineStore`（首编译 vs 增量基线版本化）+ `D9PerfEvaluator.Compare`（软 1.2x / 硬 1.5x 阈值）+ `CodingDoDGateEvaluator.EvaluateD9Async` 承载。D9 **不参与** `Completed` 判定链（`RunAutoGatesAsync` 不含 D9——D9 是阶段 E 增强门，经 `EvaluateAsync(AIDoDGateKind.D9Perf)` 单独调用）。D9 编号与 D8 真实接入冒烟门正交；阶段 E 交付以场景五面推演闭环为准（[scenario-drive-acceptance](scenario-drive-acceptance.md)），非测试全绿。

---

[返回 043(../../043-harness.md) · [AIRfc](airfc.md) · [llm-gates](llm-gates.md) · [纠偏协议](anchor-correction-protocol.md)
