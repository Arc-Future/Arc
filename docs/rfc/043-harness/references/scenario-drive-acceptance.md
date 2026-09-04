# 场景闭环推演验收协议

> **注（2026-08-29）**：原 `crates/arc-integration` 已退场（a2627a0f）。本文所引
> `cargo test -p arc-integration ...` 验证命令不再可用；现行验证矩阵为
> `cargo test --workspace`（运行时面 `cargo test -p arc-tests --features full-rt`），
> 详见仓库根 `CHANGELOG.md`。

> 关联 [043 Coding Agent Harness 工程(../../043-harness.md)（宣称纪律 · 分层）· [真实场景运转协议](scenario-operation.md)（38 场景矩阵）· [可执行 DoD](definition-of-done.md) · [AIRfc](airfc.md) · [回合小结](work-summary.md) · [并行子代理（P3）](parallel-subagents.md) · [LLM 门闩](llm-gates.md)。
>
> 本子项把 [scenario-operation](scenario-operation.md) 的**场景判定**升级为**可执行的验收判据**：不以测试全绿为准，而是对每个场景输入 Harness 后，按「真实代码逻辑 + LLM 回复视角 + 工具调用 + 提示词/上下文信息」共同推演，判定能否形成「需求 → 计划 → 实现 → 验证 → 验收」闭环。**场景闭环推演是 Harness 交付的判据；e2e 只是证据面之一。**

## §0 定位与宣称门闩

本协议是**文档协议层**（不改 `.as`、不参与 DoD 判定），但自身同样守宣称纪律：

| # | 门闩 | 未过时 |
|---|------|--------|
| 1 | **B 面必须落在真实存在的代码上**：推演引用的组件 / 命令 / 分支 / 行号必须只读核对属实；设计态 / 空想部件一律标注并直接判「断点」 | 禁以设计态冒充真实 |
| 2 | **判定以五面推演闭环为准**：测试 / e2e 全绿只是 B 面证据之一，不是判定本身；e2e 全绿但某场景推演断链（缺工具 / 缺上下文 / 组件空想）→ 该场景仍不达交付 | 禁「e2e 绿 = 场景交付」 |
| 3 | **每场景必须标闭环 / 断点**：闭环 ✅ 或断点 ❌（注明断在哪一面：B 空想 / C 卡壳 / D 无返回 / E 缺信息）；断点场景只可写「部分 / 进行中」 | 缺标视为未完成 |
| 4 | **推演卡随修复更新**：阶段 A–H 任一修复项落地后，所属场景**重推演**，断点消除才可宣称该场景「交付达成」 | 禁「修复登记 = 交付达成」 |

## §1 推演方法（核心定义）

每个场景的「闭环判定」= **五面共同推演**：

| 面 | 推演什么 | 判据 |
|----|----------|------|
| **A. 场景输入** | 用户话语（原话） + 项目状态（空目录 / 现有项目；有无 `.arcgr` / 测试 / Wiki / conventions / AIRfc） | 输入要具体、可复现 |
| **B. 真实代码路径** | 该输入触发 Harness 哪个真实组件 / 命令 / 分支（如 `/rfc` → `DirectionLoop.RfcAsync` → `AIHarnessSession.SetRfc` → `AIRfcRuntime.Create` → `AICoordinator` 租约） | **必须落在真实存在的代码上**（标注文件与行号）；无真实代码 = 空想 = 直接断点 |
| **C. LLM 回复视角** | 在「`CodingAgentPrompt` 系统提示 + 注入的上下文」下，模型会如何理解、如何复述意图、如何选择下一步（UNDERSTAND → EXPLORE → DECOMPOSE → PLAN → IMPLEMENT → VERIFY → REPORT） | 给出代表性回复摘要；模型「卡壳 / 只能问人 / 只能猜」即断点 |
| **D. 工具调用序列** | 模型按提示纪律会调用哪些工具（`read_file` / `grep_search` / `git_status` / `plan` / `arc_build` / `arc_test` 等），每个工具**真实返回值是什么、卡在哪** | 工具存在且返回可消费信号；「工具调用无返回 / 返回不可消费」即断点 |
| **E. 提示词与上下文信息** | Instructions（身份 / 方法论 / 工具纪律 / 验证纪律 / 输出契约）、AIRfc 锚点、Wiki 知识面、conventions Rules、能力白名单（`quality.Verify` / `fs.*` / `shell.Run`）如何注入、够不够 | 上下文足以支撑 C 面决策；「缺信息 → 模型卡壳」即断点 |

**闭环判定**：五面推演走完后，场景是否形成「需求 → 计划 → 实现 → 验证 → 验收」闭环。**任何一面断裂即判定该场景未达交付**：

| 断点类型 | 含义 | 判定口径 |
|----------|------|----------|
| B 空想 | 引用的组件 / 命令在真实代码中不存在（如 `arc new`、`DetectProject`、`/status`、`RecordFixAttempt` 有定义零调用） | 直接断点 |
| C 卡壳 | 模型在当前提示 + 上下文下无法决定下一步（如模糊需求无澄清向导、空目录无参考源） | 断点 |
| D 无返回 | 工具真实调用无数据 / 返回不可消费（如 `arc inspect` 对真实项目 K1–K3 返回 symbols=0 或崩溃） | 断点 |
| E 缺信息 | 所需上下文未注入（如 AIRfc 锚点 `AttachRfcToInstructions` 零调用、Acceptance 纯文本无法承载结构化验收） | 断点 |

## §2 推演模板（38 场景通用）

每场景一张**推演卡**：

```text
场景 X.Y <名> · 判定门槛：闭环 / 断点
[A 输入]  用户话语：…；项目状态：…
[B 真实代码路径] 命令 / 组件 / 分支序列（逐项标注：真实现 / 设计态 / 空想）
[C LLM 视角]  模型会怎么回复 / 理解 / 决策（代表性回复摘要）
[D 工具调用]  调用序列：工具 → 真实返回值 → 下一步（标注卡点 / 断点）
[E 上下文]   注入的提示词 / 锚点 / Wiki / conventions / 能力（标注缺失）
[闭环结论]  闭环 ✅ | 断点 ❌（断在哪一面：B 空想 / C 卡壳 / D 无返回 / E 缺信息）
[交付判据]  该场景可宣称「交付达成」需满足的推演闭环条件
```

## §2.1 设计定稿前强制迷你推演卡（B1 前置）

> **把五面推演前置到设计前，而非实现后补**（来源 [harness-llm-lessons B1](harness-llm-lessons.md) · [design-review B5 门闩](design-review.md)）。设计定稿（AIRfc Design 落 Spec / AIPlan 批准 / 任一新增抽象、状态、字段）前，先做一张**迷你推演卡**：

```text
设计项：<抽象 / 状态 / 字段名>
[A 输入]  踩到它的真实场景（用户话语 + 项目状态；必须 ≥1，写不出即不批准）
[B 路径]   该场景触发哪条真实代码路径用到它（标注：真实现 / 设计态；设计态即不批准）
[C 视角]   模型在提示 + 上下文下如何触达并消费它（触达不到即不批准）
[结论]     踩到 ✅（可设计）| 空想 ❌（不批准，砍掉或降级到被真实场景驱动后再设计）
```

- **强制前置**：每新增抽象/状态/字段先填一张卡；**写不出「≥1 真实场景踩到」→ 不批准该抽象**（对 `design-review` 必须项「防过度设计」）。
- **与 §3 完整推演卡的关系**：迷你卡是设计前的轻量闸门（五面可精简）；§3 完整卡是交付判据（五面完整闭环）。设计前先过迷你卡，交付时再过完整卡。

## §3 L0 核心场景（能力面 × 诚实缺口）

> 六张 L0 推演卡（1.1 / 1.2 / 2.1 / 2.3 入口 + 3.4 / 4.1 验收）按 §2 模板把「需求 → 计划 → 实现 → 验证 → 验收」的收敛能力收敛为**能力面 + 诚实缺口**；逐卡操作回放不重复列写，各阶段验收按 §5 应用规则对具体场景实时推演判定。

| 场景 | 能力面（真） | 诚实缺口 |
|------|------------|---------|
| 1.1 空白目录 · 模糊需求 | `/rfc` 澄清向导（`ClarifyAsync` 交互追问 → `airfc:clarify` → `ReviseRfc` 落 Spec）+ **Acceptance 先行硬门**（`AttachPlan` 无验收断言即拒）+ **AIRfc 锚点注入**（`AIRfcContextProvider` Rules 层活块，前缀稳定吃 KV cache）+ design-review 轻量版提示 | design-review 硬门为轻量提示版（完整清单门未建）；「先立项后 refine」路径下用户拒绝时计划保持门闩 |
| 1.2 空白目录 · 项目类型 / 技术选型 | `arc new <dir> [--name <pkg>] [--agent]` 生成最小可编译骨架（arc.toml + Program.as + 可选 README.md）+ `arc detect <dir>` 分类（uninitialized / arc_project / coding_harness / domain_two）+ **默认 Skills**（`conventions.agent.md` 内嵌模板 → `--agent` 落 `.arcagent/conventions.md` → `ProjectConventionsProvider` Rules 注入） | 模型侧经 run_command 调 CLI（无专用 [AITool] quality.arc_detect / arc_new，后续可选接线）；`--agent` 骨架为「依赖声明 + conventions 模板」，不生成完整组合根 |
| 2.1 现有项目 · 模糊需求 | **D1 语义门**经 `arc inspect` 返回真实引用数据（K1 顶层符号 + Main 入口 / K2 项目模式多文件符号合并 + 跨文件 edges / K3 MethodCall 边）+ conventions / Wiki 注入真 | K5 增量粒度、K7 warning 判定未落地（沿用 [definition-of-done 已知限制](definition-of-done.md) 表）；D1 判定子进程有独立 flaky（栈/环境相关） |
| 2.3 现有项目 · 修 bug | **L2 自动迭代 ≤3 轮机器闭环**（`RunFixLoopAsync`：D0→D3 门链 → `AIDoDFixFeedback` 结构化回喂 → RecordFixAttempt 计数 → 超限 `CheckpointRollbackAsync` 回滚 + 升级人；`FixBudgetExceeded` 显式暴露） | D0 `--message-format json` 结构化诊断（SR-2）与 D3 断言 diff 解析（B6/A.7）未落地——回喂载体为原文文本 |
| 3.4 方向错推倒重来 | **多绿点历史**（index.json + checkpoint-<seq>.json）+ 大文件内容寻址副本 + 回滚联动 AIRfc（`RestoreRevision`）/ AIPlan（`RestoreStatus`）+ `/rollback --cp` 指定绿点 | 门状态无持久化面（下次 /dod 重跑重对齐）；大文件为全文副本（非增量 diff）；AIRfc/AIPlan/门状态跨会话持久化属后续 |
| 4.1 验收功能对不上 | **Acceptance 结构化**（`AIAcceptanceSpec.Items` + `/revise --acceptance/--test` 落结构化）+ **D5 证明机器校验**（`D5ProofVerifier` 文件存在 / `arc test --list-tests` 测试名可解析；Unchecked/Invalid 标红而非 Passed）+ **D3 用例级明细**（`--logger json` 解析 passed>0 且 failed==0 + 防降级基线 + TestName 对照） | D5 深度校验（证明「真实运行通过」而非仅引用存在）与 D3 断言级 diff（B6/A.7）为增强面；防降级基线为会话内记忆（跨会话持久化属阶段 F） |

## §4 与既有验证的关系

| 面 | 角色 |
|----|------|
| 测试 / e2e（`arc_ai_*` 系列、`cargo test -p arc-integration`） | **证据面之一**：佐证 B 面的真实代码路径确实存在且可运行（如 checkpoint e2e 证明 `CheckpointRollbackAsync` 真实回滚）；是推演的输入，不是判定本身 |
| 场景闭环推演（本协议） | **判定**：以五面推演闭环为准。e2e 全绿但某场景推演断链（缺工具 / 缺上下文 / 组件空想）→ 该场景仍不达交付 |

判例口径：每个场景是否达交付由 §3 能力面 / 诚实缺口映射的五面推演闭环判定；e2e 仅佐证 B 面真实代码路径存在可运行，不构成交付判定本身。

## §5 应用规则

1. **阶段 A–H 每个修复项验收 = 用本协议推演其所属场景是否闭环**，而非只跑测试。修复落地 → 更新对应推演卡（重推演）→ 断点消除才宣称该场景「交付达成」；未消除 → 该场景维持断点 ❌。
2. **空想部件（无真实代码）在推演中直接判断点**：B 面引用的组件必须是「grep / 只读核对」可证实的代码（文件 + 行）；凡只存在于文档 / 设计态（如 `AIMergeTransaction`、`TotalBudget` 超限收束、`/status`、`/retro`、`arc baseline`）一律标「空想 / 零调用」并直接判断点。
3. **推演卡与场景矩阵同步**：[scenario-operation](scenario-operation.md) 的场景判定（能力面 / 诚实缺口）是推演卡的输入基线；本协议把判定细化为五面断点定位。矩阵判有缺口的场景在断点消除前不得宣称「交付达成」。
4. **宣称纪律对齐**：本协议门闩与 [llm-gates](llm-gates.md) / [airfc](airfc.md) §0 同一精神——未经验收不得宣称；本协议给出「验收 = 五面推演闭环」的可执行口径。
5. **38 场景全覆盖**：六张 L0 推演卡为模板与基准；其余 32 场景（1.3 / 2.2 / 2.4 / 3.1–3.3 / 3.5 / 4.2 / 4.3 / A.1–A.10 / B1–B12（含并入 B3 的「git 分支迭代与合并」深度子面））按同一模板推演，作为各阶段验收时的逐场景判定依据（不重复列全卡）。两大机制子项（[subagent-management](subagent-management.md) 方案 A / [conflict-branch](conflict-branch.md) 方案 B）的演进路径 A1–A5 / B1–B4 每步验收 = 对应场景（3.1/3.2/3.3/A.1/A.6/A.9/B3/B3′/B11/B7）五面推演闭环。

## 附：B 面真实代码路径索引（推演锚点）

> 推演引用组件须落在此表或等价真实代码上；表外引用须先核实。

| 能力面 | 真实组件（文件:行） |
|--------|---------------------|
| REPL 斜杠路由 | `examples/ArcAgent/ArcAgent/Repl/ReplCommands.as:67`（/rfc /revise /reject /summary /checkpoint /rollback /dod /plan /approve /memory /sessions /resume /task） |
| 方向环命令 | `examples/ArcAgent/ArcAgent/Repl/DirectionLoop.as:39`（TryHandleAsync）· `:81`（RfcAsync）· `:120`（ClarifyAsync 澄清向导）· `:178`（ReviseAsync）· `:310`（RollbackAsync）· `:355`（DodAsync）· `:434`（D7AcceptAsync）· `:399`（PrintD5） |
| D5 证明槽位 | `examples/ArcAgent/ArcAgent/Repl/D5SelfReview.as:47`（SetProof 存证明 + 同步文件引用校验；`ValidateProofsAsync` 委托 `std/AI/Agent.Harness.Coding/DoD/D5ProofVerifier.as` 机器校验——文件存在 / 测试名 `arc test --list-tests` 可解析；Unchecked/Invalid 标红）· `:82`（Render [✓]有效 / [~]未校验 / [✗]无效） |
| 协作确认点 | `examples/ArcAgent/ArcAgent/Repl/CollaborationCheckpoints.as:14`（DetectAsync：std/ 与删除核心路径判高风险） |
| Harness 薄壳 | `std/AI/Agent.Harness/Host/AIHarnessSession.as`（SetRfc / ReviseRfc / RejectRfc / AttachPlan（Acceptance 先行门闩）/ EnableAcceptanceGate / RecordClarify（airfc:clarify）/ AcceptanceDefined·DesignDefined / CheckpointGreenAsync / CheckpointRollbackAsync / CompletePlanAfterDoDAsync） |
| AIRfc 锚点注入 | `std/AI/Agent.Harness/Context/AIRfcContextProvider.as`（Rules 层活块；Program.as 组合根 AddProvider 注册——/rfc /revise 后锚点进模型请求，块内容仅随 Revision 变更 → 前缀稳定吃 KV cache；e2e 断言请求体含 `[airfc RFC-* vN]` 块） |
| AIRfc 运行时 | `std/AI/Agent.Harness/Rfc/AIRfcRuntime.as`（Create / Revise / RejectRfc / TryBeginWrite（RfcSpec 租约））· `std/AI/Agent.Harness/Rfc/AIRfc.as`（ToContextBlock）· `AIAcceptanceSpec`（结构化 Items，纯文本兼容两态） |
| DoD 编排 | `std/AI/Agent.Harness/DoD/AIDoDOrchestrator.as`（RunGatesAsync D0–D3 门链 / RunFixLoopAsync（maxRounds=3：结构化回喂 → RecordFixAttempt 计数 → 修复轮 → 重跑门）/ RecordFixAttempt / FixBudgetExceeded / RunAutoGatesAsync / AllPassed（Pending≠Passed）/ RunAggregatedGatesAsync 汇总门）· `AIDoDFixFeedback.as`（结构化回喂）· `AIDoDFixLoopResult.as`（Passed/BudgetExceeded 携带 FixRounds）· `IAIFixRoundProvider.as`（修复回合注入点）；L2 闭环：`AIHarnessSession.RunFixLoopAsync`（绿点前置 + 超限回滚升级）· `ReplFixRoundProvider.as` |
| Coding 门判定 | `std/AI/Agent.Harness.Coding/DoD/CodingDoDGateEvaluator.as`（D0 arc build 退出码 / D3 arc test `--logger json` → `D3TestReport` 用例级明细解析：passed>0 且 failed==0 才绿 + 防降级基线 + Acceptance TestName 对照 / D1 inspect + explain（K1–K3）/ D2 契约扫描 / D4 diff 覆盖 / D6 反模式） |
| quality 工具 | `std/AI/Agent.Harness.Coding/Quality/QualityTools.as`（arc_build / arc_test / arc_check / arc_inspect / arcgr_query，能力 `quality.Verify`）· `QualityCli.as`（IsGreen 仅 exit=0，D0 用；D3 断言绿经 `D3TestReport` 判定）· `DoD/D5ProofVerifier.as`（D5 证明引用存在性校验） |
| 绿点 / 回滚 | `std/AI/Agent.Harness/Checkpoint/AICheckpointStore.as`（CaptureAsync，多绿点：index.json + checkpoint-<seq>.json + objects/<sha256>.bin / RollbackAsync，按 Id/seq/最近指定绿点）· `std/AI/Agent.Harness/Host/AIHarnessSession.as`（CheckpointRollbackAsync 联动 AIRfc/AIPlan）· `AIRfcRuntime.RestoreRevision` · `AIPlan.RestoreStatus` |
| fs / repo / shell 工具 | `examples/ArcAgent/ArcAgent/Tools/FSTools.as`（read_file / list_dir / search_text / write_file / edit_file）· `RepoTools.as`（grep_search / git_status / git_diff）· `ShellTools.as`（run_command，HITL） |
| 系统提示工程 | `examples/ArcAgent/ArcAgent/Context/CodingAgentPrompt.as`（Method 1–7 阶段 / ToolDiscipline / Verification ≤3 fix rounds 文本 / OutputContract） |
| 组合根 / 上下文 | `examples/ArcAgent/ArcAgent/Host/AgentHost.as`（BuildOptionsAsync 能力白名单 / PlanGatedCapabilities / conventions provider 注册）· `examples/ArcAgent/ArcAgent/Context/AgentContext.as`（Wiki 注入）· `ProjectConventionsProvider.as`（conventions → Rules） |
| 脚手架 / 项目识别 | `crates/arc/src/scaffold.rs`（`scaffold_project` 生成 arc.toml/Program.as/README.md/`.arcagent/conventions.md`；`detect_project` 分类 uninitialized/arc_project/coding_harness/domain_two）· `crates/arc/src/main.rs`（`arc new` / `arc detect` 子命令，detect 支持 `--format json`）· 默认 Skills 模板 `crates/arc/templates/conventions.agent.md`（`include_str!` 内嵌进 arc 二进制，`--agent` 落盘到新项目 `.arcagent/conventions.md`，ProjectConventionsProvider 消费） |
| 已知断点（阶段 A 输入） | K1–K3（`.arcgr` 项目级）· K4（Main 冲突）· K5（文件级增量）· K6（D4 三失效）· K7（warning 未判定）· K8（传参姿势），见 [definition-of-done 已知限制](definition-of-done.md) |

> **两大机制 B 面锚点口径**：**已具备**——方案 A：`AISubAgentState` / `AISubAgentMessage` / `AISubAgentDecision`（[subagent-management](subagent-management.md) A2/A3）；方案 B：`AIConflictRecord` / `AIConflictResolver` / `AISpecConflictDetector`（[conflict-branch](conflict-branch.md) B1）——推演时以真实代码路径为准。**仍设计态（诚实缺口，不入 B 面锚点）**——方案 A：`AISubAgentManager` / `AISupervisionPolicy` / `AISubAgentBudget`；方案 B：`AIBranch` / `AIBranchLease` / `AIMergeController` / `AIMergeTransaction`——推演时须标「空想 / 设计态」并判断点。

---

[返回 references 索引](index.md) · [返回 043(../../043-harness.md) · [真实场景运转协议](scenario-operation.md) · [可执行 DoD](definition-of-done.md) · [AIRfc](airfc.md) · [LLM 门闩](llm-gates.md)