# 真实场景运转协议（Harness → 真实交付场景）· 方法学契约（38 场景全景）

> 关联 [043 Coding Agent Harness 工程(../../043-harness.md)（双环 · AIRfc · D0–D7）· [AIRfc](airfc.md) · [可执行 DoD](definition-of-done.md) · [并行子代理（P3）](parallel-subagents.md) · [纠偏协议](anchor-correction-protocol.md) · [测试先行](testing-first.md)。
>
> 本子项是 **RFC 043 的协议层契约**：把「双环 + AIRfc + D0–D7」的**能力契约**放到真实交付场景里跑，检验**组件协作是否真、闭环是否闭合、缺口在哪**。由**原两路只读推导（14 场景）** + **A/B 两组行业方法论只读推导（10 + 12 场景）** + **运行中子代理干预（3.5）** / **git 分支迭代与合并（并入 B3）** 两路纵深推导，产出 **38 个真实场景**（入口 7：空白目录 3 + 现有项目 4；执行与验收 8：3.1–3.5 + 4.1–4.3；A 组 10：需求/变更/协作域；B 组 12 + 并入 B3 的「git 分支迭代与合并」深度子面：执行/质量/治理域）的完整判定。每场景按**能力面 + 诚实缺口**表述；判定以外推与文档契约为准。**不改写任何结论。**

## §0 宣称门闩（本协议的履约纪律）

本协议是**文档协议层**（不改 `.as`、不参与 DoD 判定），但自身同样守宣称纪律：

| # | 门闩 | 未过时 |
|---|------|--------|
| 1 | **场景判定与实现一致**：判定所引用的代码 / 文档 / e2e 证据必须真实存在（只读核对），不把文档宣称当实现 | 不得宣称对应能力「完成 / Completed / 已收敛」 |
| 2 | **每场景必须标注能力面 / 缺口**：区分「能力成立」与「缺口 / 设计态 / 体系空白」 | 缺标视为未收敛 |
| 3 | **不把「部分」当「完成」**：存缺口的场景只可写「部分 / 设计态」，缺口必须入 §5 汇总与 §6 目标面，禁止用完成语掩盖 | 禁「部分即完成」表述 |
| 4 | 判定含「待验证」的部件不得按「已验证」宣称 | 禁以登记代复验 |

> **交付判据**：本矩阵每场景是否可宣称交付，以 [场景闭环推演验收协议](scenario-drive-acceptance.md) 为准——对每场景做五面推演（A 输入 / B 真实代码路径 / C LLM 视角 / D 工具调用 / E 上下文），形成「需求 → 计划 → 实现 → 验证 → 验收」闭环才算交付；**非测试全绿**。e2e 只是 B 面证据之一。存缺口场景在断点消除前不得宣称「交付达成」。

## §1 场景全景矩阵（38 场景）

> 原 14 场景（1.1–4.3）六段式判定见 §2–§3（结论原样保留）；A 组 10 场景见 §4.1；B 组 12 场景见 §4.2；**运行中子代理干预（3.5）见 §3.5；git 分支迭代与合并（并入 B3 深度子面 B3′）见 §B3′**；A/B 组与原有场景的并入 / 新门类映射见 §4.0。域：入口 / 执行 / 验收 / 质量 / 治理。

| # | 场景 | 域 | 能力面 | 核心缺口（一句话） |
|---|------|----|--------|--------------------|
| 1.1 | 空白目录 · 模糊需求 | 入口 | 收敛协议：`/rfc` 澄清向导 + Acceptance 先行门 + AIRfc 锚点注入 | design-review 硬门为轻量提示版 |
| 1.2 | 空白目录 · 项目类型/技术选型 | 入口 | 脚手架能力：`arc new`（骨架生成）+ `arc detect`（项目识别）+ 默认 Skills（`Skills/conventions.md` → `.arcagent/conventions.md`） | 无专用 [AITool] `quality.arc_detect` / `arc_new`；`--agent` 骨架不含完整 AgentHost 组合根 |
| 1.3 | 空白目录 · 多轮需求讨论 | 入口 | 升版轨迹真 | acceptance 纯文本，无自动锁测 |
| 2.1 | 现有项目 · 模糊需求 | 入口 | Wiki/conventions 注入真；`.arcgr` 项目级引用面（顶层符号 + `Main` 入口 / 跨文件符号 + New/Call edges / MethodCall 边收集）给 D1 语义门供数 | K5 增量粒度 / K7 warning 未判定 |
| 2.2 | 现有项目 · 审查进度 | 入口 | 门实时输出真 | 无 `/status` 聚合、门不持久 |
| 2.3 | 现有项目 · 修 bug | 入口 | L2 自动迭代接线——`RunFixLoopAsync` 消费 `RecordFixAttempt`（≤3 轮机器判定）+ 结构化回喂 + 超限回滚绿点升级人 | D0 结构化诊断 / D3 断言 diff 解析（设计态） |
| 2.4 | 现有项目 · 继续推进 | 入口 | transcript/Wiki/绿点真；AIRfc / AIPlan 可序列化重建 | 门状态不持久（下次 `/dod` 重跑重算） |
| 3.1 | 执行中纠偏 | 执行 | 并行纠偏广播——`/revise` 升版 → revision-changed 广播 → 在飞检查点 + 重建 ContextBlock 到新 Revision + 租约重验（Scope 变冲突 → Failed；不变 → 继续） | 多事件混合仲裁（设计态）、REPL 无并行管理器实例 |
| 3.2 | 并行任务踩踏 | 执行 | 冲突织物拦截 + reconcile 循环化 + 租约惰性化（假冲突消除）+ 失败/死亡即时释放租约 + 步数预算守护最小版 | `TotalBudget` 完整收束 / 真并行（设计态） |
| 3.3 | 半途改需求增需求 | 执行 | 升版闭环真；工作项动态增项（`graph.AttachItem` 运行中增项 + `SpawnAsync` 派发 + `SetParallelism` 弹性） | 验收测试同步人工、工作项重跑处置（`Priority`/`Reprioritize`/`Invalidate`）缺 |
| 3.4 | 方向错推倒重来 | 执行 | 多绿点历史（index.json + checkpoint-`<seq>`.json）+ 大文件内容寻址副本真实恢复 + 回滚联动 AIRfc/Plan | 门状态无持久化面（/dod 重跑） |
| 3.5 | 运行中子代理干预 | 执行 | 并行容器 / 升版闭环 / 冲突织物为底座；停止收束（`CancelPendingAsync` 取消未启动 + 中断在飞 + 即时释放租约）、决策同步（广播 / 定向 / 旁路注入 / revision-changed 重对齐 + 租约重验）、拉起（动态增项 + 派发 + 弹性并行度） | 多事件仲裁（S-6）体系空白；`Revise:90` WorkItems 别名、REPL 零接线 |
| 4.1 | 验收功能对不上 | 验收 | Acceptance 结构化 + D5 证明机器校验（文件/`--list-tests` 引用存在性）+ D3 用例级明细（`--logger json` 解析 + 防降级基线 + TestName 对照） | D5 深度校验、D3 断言级 diff（设计态） |
| 4.2 | UI 效果差距 | 验收 | — | D0–D7 无 UI 视觉 / 交互门（体系空白） |
| 4.3 | 测试不通过 | 验收 | D3 `--logger json` 真 + 含 `Main` 项目可测；L2 自动迭代接线（`RecordFixAttempt` 消费） | D3 断言级 diff 解析（设计态） |
| A.1 | 需求来源冲突 · 多方向并存 | 治理 | 并发租约拦截 + 顺序矛盾判定（字段级 diff：异来源覆盖同 acceptance 项 → Contested + `AIConflictRecord`）+ 人 CCB 裁决（`/conflict` 列出 + `ResolveAsync` 必须 `resolvedBy` 禁自动选胜者 → 新 Revision 基线） | Owner/Priority 字段（RTM 追溯） |
| A.2 | 需求中途冻结 · 冻结窗口（CCB） | 治理 | Revision/Superseded 基线骨架；`Frozen`/`Closed` 等状态入枚举 | CCB 裁决（AICR）/ 影响分析门 / `/freeze` REPL（设计态） |
| A.3 | 范围蔓延 Scope Creep | 治理 | D4 越界单点真 | 无范围度量 / 蔓延阈值门 / NonGoals 边界面 |
| A.4 | 技术债与遗留代码 · 接手建基线 | 入口 | D6 债扫描 + 约定/Wiki 注入 | 无 `arc baseline` / characterization / 债登记 |
| A.5 | 依赖库/框架升级 · 跨切面变更 | 执行 | ABI 人确认点 + 绿点回滚真 | 影响分析有方法边数据但无 SemVer/ABI 校验、子图回归不可行 |
| A.6 | 多人/多 Agent 同仓库协作 | 治理 | 单宿主冲突织物真 | 跨进程织物空、基线裁决空、`AIMergeTransaction` 设计态 |
| A.7 | 非功能需求进 Acceptance 与 D3 | 验收 | `AIDoDGateKind` 扩展机制 | 无基准测试 / 安全扫描 / 可访问性门、Acceptance 无量化面（D9 门设计态） |
| A.8 | 验收环境/数据问题 | 验收 | 「环境不可用→升级人」规则 + e2e skip 真 | 环境就绪门 / mock 分级 / 测试数据管理缺 |
| A.9 | 人为撤单 / 用户放弃任务 Cancel | 执行 | 撤单语义 + 在飞收束（`AIRfcStatus.Cancelled` / `AIRfcWorkItemStatus.Cancelled` / `CancelPendingAsync` 收束 + 修取消被吞） | `/cancel` REPL 接线 + keep-wip/rollback 处置选项缺 |
| A.10 | 复盘与回顾 Retrospective | 治理 | 逐单元小结 + 决策轨迹真底座 | 无聚合 / 度量 / 反馈环 |
| B1 | CI/流水线集成 | 执行 | DoD 判定器（`AIDoDOrchestrator.RunAutoGatesAsync`）在 REPL 会话内可跑 | headless 产物 / CI 适配 / flaky 与超时策略（体系空白） |
| B2 | 回归风险与回归测试策略 | 质量 | 分级解锁契约真 | 无影响图、增量仅文件级、无回归基线、防降级无 enforcement |
| B3 | 并行开发与主分支保护 | 执行 | 冲突织物后到拒绝真；分支模型 + 分支级绿点 + `AIMergeTransaction` 两阶段提交；`AIPlan.Id` 稳定租约键 | 合并门 CI/PR + 三方裁决（设计态）；跨进程共享织物缺 |
| B3′ | git 分支迭代与合并（并入 B3 深度子面） | 执行 | 6 细分中**分支模型 / 合并绿点 / 合并事务回滚**；`AIPlan.Id` 稳定租约键 | 合并门（汇总门 = CI/headless + 人评审 D7）与「合并时冲突裁决」（三方视图 + 人 CCB）仍设计态 |
| B4 | 发布/里程碑管理 | 治理 | — | Release 聚合、版本号、Changelog、发布门、灰度/回滚（全空） |
| B5 | 生产事故与紧急修复（Hotfix） | 治理 | 快速升版/回滚/升级人真 | 严重度分级、绕行通道、事后补测补文档闭环、事故登记（缺） |
| B6 | 测试质量治理 | 质量 | D3 `--logger json` 真、含 `Main` 项目可测 | 只退出码、无 flaky/同源/降级检测、无覆盖率 |
| B7 | 性能与可观测性门 | 质量 | 决策轨迹可观测真、编译器性能基线有 | 运行期可观测门与基准门（D9）仍设计态 |
| B8 | 安全与合规门 | 治理 | 运行时 fail-closed 面真 | DoD 无 secret/依赖/交付能力审计 |
| B9 | 多项目/工作区管理 | 执行 | 项目级隔离真 | 无跨项目 AIRfc / 共享预算 / 多实例协调（明确单进程非目标） |
| B10 | 团队规模扩展 | 治理 | 确认点 + `approval` 事件轨迹真 | 无身份/角色/审批链，SR-3 假确认口子未收 |
| B11 | AI 能力边界与降级 | 执行 | L2 升级人路径真；超时/预算强制收束梯 | 能力探测/幻觉检测/失败隔离缺 |
| B12 | 知识沉淀与复用 | 治理 | Wiki/conventions 注入真、决策轨迹真 | 无知识提取回写、无跨任务学习 |

> **闭环纪律**：各场景的「能力面 / 诚实缺口」见各节对断面；未标注「闭环」者不得宣称能力已闭环，部分（⚠️）/ 缺口（❌）场景的诚实缺口以各场景缺口列传述为准（A/B 组只读结论见 §4）。门闩下不得把任何「部分」当完成；声明「闭环」的场景以推演卡五面闭环为准（[scenario-drive-acceptance §3.1–§3.6](scenario-drive-acceptance.md)）。

---

## §2 入口场景（1.1–2.4）

### 1.1 空白目录 · 模糊需求

- **用户动作**：在空白目录给出一句话模糊需求（无验收标准、无范围、无技术约束）。
- **运转流程**：`/rfc <一句话意图>` 立项 → Revision 1；Design/Acceptance 空缺 → **澄清向导**追问「验收标准 / 影响面·边界 / 是否接受先立项后 refine」→ `airfc:clarify` 决策事件 → 确认后 `ReviseRfc` 落 Spec（Revision+1）→ **Acceptance 先行门**（无验收断言 AttachPlan 被拒）→ AIRfc 锚点（`AIRfcContextProvider` Rules 活块）进模型 Instructions → `/dod` 验收。
- **组件协作**：`DirectionLoop.RfcAsync` → `ClarifyAsync`（澄清向导）→ `AIHarnessSession.SetRfc` / `RecordClarify` / `ReviseRfc` / `AttachPlan`（门闩）→ `AIRfcRuntime.Create/Revise`（RfcSpec 租约 + Revision）→ `AIRfcContextProvider`（锚点注入，组合根注册）→ `D5SelfReview`。
- **能力面**：命令链为真；锚点注入 / 澄清向导 / Acceptance 门闩为真（请求体含 `[airfc RFC-* vN]` 块 + AttachPlan 门闩 + `airfc:clarify` 事件）。
- **诚实缺口**：design-review 硬门为轻量提示版（完整清单门待排期）；Acceptance 结构化（场景 + 断言条目，消除纯文本面）未做。
- **契约对照**：043 §2「意图/设计/验收必填 Spec 面」与 [testing-first](testing-first.md)「验收测试先行锁定」——收敛协议已机器引导（向导追问 + 门闩强制）；验收测试自动锁测仍未接线（同 1.3 缺口）。
- **能力面（补件）**：① `/rfc` 澄清向导（追问验收场景 → 产出 Acceptance 条目）；② Acceptance 先行门（未定义 Acceptance 禁止进入实现宣称）；③ design-review 硬门——⚠️ 轻量提示版，完整清单门待排期。

### 1.2 空白目录 · 项目类型/技术选型

- **用户动作**：在空白目录期望 Harness 识别项目类型、落地骨架、按类型装配默认能力。
- **运转流程**：`arc detect <dir>`（未初始化 → 提示 `arc new`）→ `arc new <dir> [--name <pkg>] [--agent]` 生成骨架（`arc.toml` + `Program.as` + 可选 `README.md`）→ `--agent` 追加 `Arc.Agent` + `Arc.Agent.Harness` 依赖并落 `.arcagent/conventions.md`（默认 Skills 模板）→ `arc build <dir>` D0 绿 → `/rfc` 正常立项。
- **组件协作**：`scaffold.rs::scaffold_project`（`arc new` 子命令）· `scaffold.rs::detect_project`（`arc detect` 子命令，human/json）· `std/AI/Agent.Harness.Coding/Skills/conventions.md`（默认 Skills 模板 → 新项目 `.arcagent/conventions.md`）· `ProjectConventionsProvider`（conventions → Rules 注入，已存在）。
- **能力面**：骨架生成 → D0 build 绿 → detect 分类 uninitialized/arc_project/coding_harness/domain_two 正确 → conventions 模板落盘。
- **诚实缺口**：模型侧经 run_command 调 CLI（无专用 [AITool] `quality.arc_detect` / `arc_new`，后续可选接线）；`--agent` 骨架为「依赖声明 + conventions 模板」，不生成完整 AgentHost 组合根代码。
- **契约对照**：043 背景把 Harness 定位为「随生态分发、开箱即用」；空白目录需有「项目识别 → 骨架落地 → 默认能力装配」真实路径。
- **能力面（补件）**：① `arc new <dir> [--name <pkg>] [--agent]` 脚手架；② `arc detect` 项目识别（`arc.toml` / 依赖边 / `.arcagent/conventions.md`）；③ 按项目类型装配默认 Skills（`Skills/conventions.md` 模板 → `.arcagent/conventions.md`）。

### 1.3 空白目录 · 多轮需求讨论

- **用户动作**：在空白目录多轮讨论需求（补充、变更、细化方向）。
- **运转流程**：多轮 `/revise --intention= / --design= / --acceptance=` → `Revision` 递增（`airfc:revised` 决策事件入轨迹）→ 旧版 `Superseded` → 门重对齐。
- **组件协作**：`AIRfcRuntime.Revise`（Revision+1、增量 Spec 更新、RfcSpec 租约）· `AIDecisionEventKindCodec`（wire 兼容）· `SessionEventLog`（transcript JSONL）。
- **能力面**：升版轨迹（Revision 递增 + 决策事件）为真。
- **诚实缺口**：**acceptance 纯文本无自动锁测**——`AIAcceptanceSpec.Assertions` 是字符串，无结构化断言，无「Acceptance → 验收测试」的自动生成 / 自动锁定接线——testing-first 的「验收测试锁定」停留在人手工。
- **契约对照**：[testing-first](testing-first.md)「锁定阶段：验收场景 → 验收测试 + 断言 → 自动验证」；实现 Acceptance 面无测试结构，D3 信号只是跑既有测试文件（`arc test`），没有从 Acceptance 自动生成 / 锁测的机制。
- **能力面（补件）**：① Acceptance 结构化（场景 + 断言条目）；② Acceptance → 验收测试自动生成 / 锁测接线（升级 D3 信号）。

### 2.1 现有项目 · 模糊需求

- **用户动作**：在现有项目（多文件、跨文件引用）给出模糊需求。
- **运转流程**：Wiki / conventions 注入上下文 → 模型只读探索 → `/rfc` 立项 → D1 语义门尝试 `.arcgr` 判定。
- **组件协作**：`AIWiki` / `ProjectConventionsProvider`（`.arcagent/conventions.md` → Rules 层）注入真 · D1 信号 `D1ArcgrInspect`（`arc inspect` / `arc explain`）。
- **能力面**：`.arcgr` 项目级引用面——K1 顶层函数/`Main` 符号收集；K2 项目模式 inspect（多文件符号合并 + 跨文件 New/Call edges）；K3 MethodCall 边收集（`resolve_method_callee` 接收者类型 → `"Class.method"` 符号解析，类方法体经 `typed_fn_symbol_name` 遍历，实例/静态/类内裸调用边真实产出）→ 真实多文件项目的引用图（symbols / edges / entry_points / reachable）有数据可判。
- **契约对照**：D1 语义完整性门宣称「引用 / 契约 / 可达性全绿」；实现 `.arcgr` 项目级能力已覆盖顶层函数 / 跨文件引用 / MethodCall 边（K1–K3 全清）。
- **能力面（补件）**：编译器 / arcgr 断点 K1–K3（§6 阶段 A）已清——K1 顶层符号 · K2 项目模式多文件 · K3 MethodCall 边。**缺口**：K5 增量粒度 · K7 warning 判定 · K8 传参姿势（沿用 [definition-of-done 已知限制](definition-of-done.md)）。

### 2.2 现有项目 · 审查进度

- **用户动作**：在现有项目审查交付进度（「现在到哪一步、哪些门过了、卡在哪」）。
- **运转流程**：**无 `/status` 命令**；`/dod` 实时逐门跑并打印（Passed / Failed / Pending）；门结果仅当次输出；门状态不落盘（进程退出即丢）。
- **组件协作**：`AIDoDOrchestrator.RunAutoGatesAsync` → `CodingDoDGateEvaluator`（D1/D2/D4/D6 真实判定）；无门状态持久化存储面。
- **能力面**：门实时输出为真（`/dod` 逐门打印，判定信号真实）。
- **诚实缺口**：**无 `/status` 聚合、门不持久**（K5/K7/K8 未清 + K7 warning 未判定 + K8 传参姿势）。
- **契约对照**：043 把 D0–D7 描述为持续可查的完成度体系；实现为一次性 REPL 输出，进程外无状态；且真实项目按当前断点几乎必红。
- **能力面（补件）**：① `/status` 聚合视图（门状态 + AIRfc + AIPlan 摘要）；② 门结果持久化（并入决策轨迹 / checkpoint 面）；③ 先清 K1–K8 断点再谈「真实项目全绿」。

### 2.3 现有项目 · 修 bug

- **用户动作**：在现有项目指派修 bug（复现失败 → 定位 → 修复 → 验证）。
- **运转流程**：`/rfc` 立项 → 模型改代码 → `/dod` 跑门 → D0–D3 失败 → **L2 自动迭代 ≤3 轮**（结构化回喂 → `RecordFixAttempt` 计数 → 修复轮 → 重跑门）→ 收敛或超限回滚绿点 + 升级人。
- **组件协作**：`AIDoDOrchestrator.RunFixLoopAsync`（`RunGatesAsync` D0→D3 门链 + `RecordFixAttempt` + `AIDoDFixFeedback` 回喂）→ `IAIFixRoundProvider.FixAsync`（REPL：`ReplFixRoundProvider` 模型回合）→ `AIHarnessSession.RunFixLoopAsync`（绿点前置 + 超限 `CheckpointRollbackAsync` 回滚）→ `CodingDoDGateEvaluator` 真实判定 → `CodingAgentPrompt` 文本引导。
- **能力面**：`RecordFixAttempt` 有消费方（`RunFixLoopAsync` 每修复轮消费）；`FixBudgetExceeded` 有消费方（超限回滚 + 升级人）；≤3 轮机器判定（判别性断言：脚本化修复收敛 FixRounds=1 / 故意不修超限 FixRounds=3 + 回滚 + 升级人）。
- **契约对照**：[definition-of-done](definition-of-done.md)「L2 自动判定：编译错误、测试失败 → 结构化断言 diff 喂回迭代 ≤3 轮」；实现已接线——回喂载体为失败门诊断文本（退出码 / `--logger json` 明细），结构化断言 diff 解析（D3）与 D0 `--message-format json`（SR-2）仍为缺口。
- **能力面（补件）**：① 失败信号 → 结构化回喂（`AIDoDFixFeedback`）；② `RecordFixAttempt` 计数 ≤3 轮；③ 超限自动回滚最近绿点 + 升级人；④ 迭代前无绿点提示 `/checkpoint`。**缺口**：D0 结构化诊断（SR-2 待排期）· D3 断言级 diff 解析（B6/A.7 同源）。

### 2.4 现有项目 · 继续推进

- **用户动作**：关闭会话后回来继续推进上次任务（`/resume`）。
- **运转流程**：`/sessions` 列出 JSONL transcript；`/resume <id>` 重放会话事件（User / Assistant / Tool / Approval / Decision）——可读历史；**AIRfc 聚合根可跨会话重建**（`/save` 落盘 `target/scratch/arcagent-state/airfc.json`，`/resume` 经 `AIHarnessSession.RestoreRfcAsync` 重建 Revision/Status/WorkItems/PlanId——非 transcript 重放冒充）；**AIPlan 树可跨会话重建**（`SavePlanAsync`/`RestorePlanAsync` 落 `target/scratch/arcagent-state/plan.json`，重建 Goal/Steps/Status/节点态/CurrentStepIndex/Revision 并回链 AIRfc.PlanId——非 transcript 重放冒充）；DoD 门状态仍不持久（下次 `/dod` 重跑重算）；绿点快照（`index.json` + `checkpoint-<seq>.json` + 大文件 `objects/` 副本）可回滚文件（多绿点可指定，3.4 已闭环）。
- **组件协作**：`SessionEventLog`（transcript JSONL）· `ReplSession` 事件钩子（决策事件转存落盘）· `AIHarnessSession.CheckpointGreenAsync`（绿点落 `target/scratch/arc-checkpoints/`：index.json + checkpoint-<seq>.json + objects/）· `AIHarnessSession.SaveRfcAsync/RestoreRfcAsync`（AIRfc 状态落 `target/scratch/arcagent-state/airfc.json`）· `AIHarnessSession.SavePlanAsync/RestorePlanAsync` + `AIPlanState`（AIPlan 树落 `target/scratch/arcagent-state/plan.json`，结构嵌套重建 ParentId）。
- **能力面**：transcript / Wiki / 绿点为真（可 resume 事件、可回滚文件）；**AIRfc 可跨会话重建**（SaveRfc → 新进程 RestoreRfc → Revision/Status/WorkItems/PlanId 重建）；**AIPlan 可跨会话重建**（SavePlan → 新进程 RestorePlan → Goal/Steps/Status/CurrentStepIndex/Revision/节点态重建 + AIRfc.Plan 回链）。
- **诚实缺口**：**门状态不持久**（每次 `/dod` 由 RunAutoGatesAsync 实时重算，诚实标注「门状态重跑对齐」）挂账后续。
- **契约对照**：AIRfc 定位「跨任务、跨版本流转」；实现已从「会话内存、无序列化」推进到「AIRfc + AIPlan 可序列化重建」；门状态仍为进程内面（无持久化，重跑重对齐）。
- **能力面（补件）**：AIPlan（含状态/步骤进度）持久化（`AIPlanState` 序列化 + `SavePlanAsync`/`RestorePlanAsync`）；门状态持久化（或 headless 门产物，阶段 F B1 同源）；绿点与 AIRfc Revision 联动（§6 阶段 C，3.4 已闭环）。

---

## §3 执行与验收场景（3.1–4.3）

### 3.1 执行中纠偏

- **用户动作**：执行中途发现方向偏差，用户或系统发起纠偏。
- **运转流程**：`/revise <理由> [--design= --acceptance=]` → Revision+1 → `AIHarnessSession.ReviseRfc` 成功后经 `RfcRevisionChanged` 钩子显式广播 `revision-changed`（`AIParallelCoordinator.PendingSyncDecisionAsync`）→ 在飞子代理**检查点中断**（`CheckpointInterrupt` → Interrupted）→ **重建 ContextBlock 到新 Revision**（`Realign`，写回基准刷新）→ **租约重验**（`AISubAgentLeaseGate.Revalidate`：Scope 变且新增写面被其它会话持有 → 冲突 → Failed + 必答小结；不变 → 继续）→ `subagent:sync` 决策事件入各 run 会话轨迹 → 门重对齐 → 纠偏事件入决策轨迹。
- **组件协作**：`AIRfcRuntime.Revise`（RfcSpec 租约、Revision+1）· `AIHarnessSession.ReviseRfc` + `RfcRevisionChanged`（广播钩子）· `AIParallelCoordinator.PendingSyncDecisionAsync` / `ApplyRevisionChanged` / `CheckpointInterrupt` / `RevalidateLease` · `AISubAgentRun.Realign` · `AIDecisionEventKind.SubagentSync` · `AIPlanGate` 计划重对齐。
- **能力面**：单会话 `/revise` 为真（升版轨迹 + 决策事件）；**P3 并行纠偏广播已成立**：Revision 升版广播给在飞子代理（检查点 + 重对齐 + 租约重验——在飞 2 子代理 → /revise 升版 → W1 Scope 不变重对齐 v2 继续 Completed（ContextBlock `[airfc v2]` 断言）/ W2 Scope 变冲突 Failed + ToolPath 必答小结 / wrap-up 旁路注入下回合生效 / `subagent:sync` 事件）。
- **诚实缺口**：多事件混合仲裁（S-6，A4/A5）；`/revise` REPL 无并行管理器实例接线（`RfcRevisionChanged` 钩子存在，由持有 `AIParallelCoordinator` 的宿主接线）。
- **契约对照**：[parallel-subagents](parallel-subagents.md)「子代理持 RfcRevision 只读快照」设计隐含「纠偏后可回收重派」；广播 / 重对齐 / 租约重验机制已成立。
- **能力面（补件）**：P3 纠偏广播——Revision 升版 → 在飞子代理检查点 + 重对齐 + 租约重验（§6 阶段 D A3）。

### 3.2 并行任务踩踏

- **用户动作**：多个工作项并行执行，写面重叠（踩踏风险）。
- **运转流程**：`AIParallelCoordinator.RunAllAsync`（reconcile 循环）：每 tick 派发新 Ready（≤ `MaxConcurrentSubAgents`）→ 在飞子代理 `RunStepAsync` 逐回合推进 → 心跳 Dead/时长熔断 → 终结同 tick 入账（Failed 必答小结）→ `ReleaseSession` **即时释放**租约 → 预算守护；子代理首次真实写（fs.Write）前经惰性租约门 `AISubAgentLeaseGate` 逐个 `Acquire(ToolPath)`（**不再派发即预取**）→ 真冲突（首写路径被占）后到拒绝 → Failed + 必答小结 → 汇总门（`RunAggregatedGatesAsync`）。
- **组件协作**：`AICoordinator`（冲突织物，ToolPath 租约后到拒绝）· `AISubAgentLeaseGate`（惰性租约门，`Arc.Agent` → sandbox 调度层 `LeaseGate`）· `AIRfcTaskGraph`（Ready / MarkInProgress / MarkDone）· `AIDoDOrchestrator.RunAggregatedGatesAsync`（汇总门唯一权威）。
- **能力面**：① **波内串行** → reconcile 循环逐回合推进（仍为 async 交错，非多线程真并行，诚实边界）；② **同波预取假冲突** → **消除**（租约惰性化：Scope 重叠但顺序写不同路径不再互伤，双 Completed；真冲突——首次真实写路径被占——仍后到拒绝）；③ **失败/死亡即时释放租约**（被占路径可被后续取得）；④ **步数预算守护最小版**（超限停止新派发 + 在飞强制收束 + 升级人），完整版 token/时长收束梯仍设计态。
- **契约对照**：「并行子代理」「`TotalBudget` 超限强制收束」「`AIMergeTransaction` 两阶段提交」（[parallel-subagents §3.3 / §3.5 / §4.5](parallel-subagents.md)）；reconcile + 惰性租约 + 即时释放已成立（§8）。
- **能力面（补件）**：① 真并行（独立宿主 / 容器，或 L2 级并行宿主另立 RFC）；② 同波预取假冲突消除（租约惰性化）；③ `TotalBudget` 超限强制收束完整版（方案 A A4）；④ `AIMergeTransaction`（staging 全落 → 统一 Move → 冲突整体回滚 + 升级人）。

### 3.3 半途改需求增需求

- **用户动作**：执行中途需求变更 / 新增需求。
- **运转流程**：`/revise` 升版 → Revision+1 → 门重对齐 → 期望旧测试标记过时、测试同步更新。
- **组件协作**：`AIRfcRuntime.Revise`（升版闭环）· `AIAcceptanceSpec`（纯文本断言面）。
- **能力面**：升版闭环为真（Revision 轨迹 + 决策事件)；**工作项动态增项**（`AIRfcTaskGraph.AttachItem` 运行中增项 + 校验重复 Id/未知依赖 + `AIParallelCoordinator.SpawnAsync` 派发原语 + `SetParallelism` 弹性并行度，见 [subagent-management §9](subagent-management.md)）。
- **诚实缺口**：**验收测试同步为人工**（Acceptance 纯文本，无自动同步测试机制）、**工作项重跑处置仍缺**（`Priority`/`Reprioritize`/`Invalidate`——已 `Done` 工作项不因 Acceptance 变化触发失效重派）。
- **契约对照**：[testing-first](testing-first.md)「纠偏时测试随 AIRfc 升版同步 + 旧测试标记过时」；实现无自动同步、无工作项重跑路由。
- **能力面（补件）**：① Acceptance → 测试自动同步（旧测标记过时入决策轨迹）；② 任务图工作项按 Acceptance 变更重跑处置（失效工作项重派）。

### 3.4 方向错推倒重来

- **用户动作**：方向错误，推倒重来（回滚到最近好状态 / 指定绿点）。
- **运转流程**：`/checkpoint` 多绿点历史（`index.json` + `checkpoint-<seq>.json` + `objects/<sha256>.bin` 大文件副本）→ `/rollback [--cp=<绿点id>]` → `CheckpointRollbackAsync` 按目标绿点（缺省最近）真实回滚（恢复差异文件、删除新建文件、大文件内容寻址恢复）→ **联动**：AIRfc Revision 恢复到绿点版本（`RestoreRevision`）、AIPlan 状态复位（`RestoreStatus`）、门状态下次 `/dod` 重跑重对齐 → `checkpoint:rollback` 事件 Detail 携带 cp id / 版本。
- **组件协作**：`AICheckpointStore`（多绿点存储 + 大文件对象存储 + 指定绿点回滚）· `AIHarnessSession.CheckpointRollbackAsync`（checkpointId 重载 + AIRfc/AIPlan 联动）· `AIRfcRuntime.RestoreRevision`（目标版 Superseded→Active；**持 RfcSpec 租约**，后到拒绝——airfc §4.2 回滚例外，不绕过冲突织物）· `AIPlan.RestoreStatus`（按记录的 PlanStatus 复位步骤）· `DirectionLoop.RollbackAsync`（`--cp=` 指定绿点）。
- **能力面**：多绿点历史 + 大文件内容寻址副本真实恢复（非仅 git 依赖；副本缺失 → 诚实 Skipped）+ 回滚联动 AIRfc/Plan 真实接线（判别性断言：回滚到第 1 绿点后文件+大文件内容恢复正确、绿点历史保留、RfcRevision=v2 恢复、PlanStatus=Approved 复位）。
- **诚实缺口**：门状态无持久化面（`/dod` 重跑重算）；AIPlan/门状态跨会话持久化（resume 重建）属阶段 C 后续项（2.4 同源;AIRfc 已持久化 S0）；大文件为全文副本（非增量 diff）。
- **契约对照**：[definition-of-done](definition-of-done.md)「绿点进决策轨迹 → 可 /resume、可复盘」；实现从「单份文件级快照 + 手动命令」升级为「多绿点 + 与方向本尊/计划联动」。
- **能力面（补件）**：① 多绿点保留 + 与 Revision 绑定；② AIRfc / AIPlan 联动回滚；③ 大文件策略内容寻址（git 或等价存储，不隐式依赖环境）。

### 3.5 运行中子代理干预

> 细分 6 场景：S-1 补充问题 / S-2 纠正需求 / S-3 停止子代理 / S-4 决策同步 / S-5 拉起新子代理 / S-6 多事件混合。**能力面 / 诚实缺口**：**S-2**（与 3.1 同根：纠偏广播，升版 → 在飞检查点 + 重对齐 + 租约重验）；**S-3（诚实缺口）**（`CancelPendingAsync` 取消未启动 + 中断在飞 + 即时释放租约 + 修取消被吞，REPL 接线仍缺）；**S-4**——`PendingSyncDecisionAsync` 广播 / `SyncDecisionAsync` 定向（revision-changed / work-item-rescope / wrap-up）+ `EnqueueMessageAsync` 旁路注入 + 租约重验；**S-1 旁路注入可用**（`EnqueueMessageAsync` 非打断注入下回合生效；定向补充问题完整仲裁仍缺）；**S-5 / S-6（诚实缺口，机制设计态）**（动态派发 / 多事件仲裁协议）。「运行中子代理干预协议」为**新门类 3.5**（执行域 · 并行干预），是 3.1 / 3.3 / A.9 已登记缺口的统一收束协议，而非发明新架构——三操作只消费既有 `AICoordinator`（租约）+ `AISession`（事件面）。机制层上位设计见 [subagent-management](subagent-management.md)（方案 A · `AISubAgentManager`，演进中）。

- **用户动作**：并行执行中用户补充 / 纠正需求，框架需能 ①停止在飞子代理 ②同步重要决策到子代理 ③拉起新子代理并行新任务。
- **运转流程**：① 升版侧真——`ReviseRfc` → `AIRfcRuntime.Revise`（RfcSpec 租约 + Revision+1 + `airfc:revised`）；② **收束侧**——`AIParallelCoordinator.CancelPendingAsync(rfcId)`：取消未启动（Open/Blocked → Cancelled，不进下一波）+ 中断在飞（联动 CTS + `AITaskRun.Cancel` + `subagent:interrupt`）+ 即时释放租约（`ReleaseSession`，路径可被后续取得）+ 回收容器（Dead）；`RunAllAsync` **不再无条件 `Complete()`**（取消被吞已修，ct 取消 → Cancelled + 必答小结，汇总门按未完结红）；失败/死亡同 tick `ReleaseSession` **即时释放租约**；③ **同步侧**——`AIHarnessSession.ReviseRfc` 成功后经 `RfcRevisionChanged` 钩子广播 `revision-changed` → `AIParallelCoordinator.PendingSyncDecisionAsync` 广播 / `SyncDecisionAsync` 定向：在飞检查点（`CheckpointInterrupt` → Interrupted）→ 重建 ContextBlock 到新 Revision（`Realign`）→ 租约重验（`RevalidateLease`：Scope 变冲突 → Failed + 小结；不变 → 继续）；`EnqueueMessageAsync` 旁路注入（soft 邮箱 → 下回合 prompt delta）；work-item-rescope 定向重取 Scope 租约；wrap-up 预算压力注入；`subagent:sync` 事件入轨迹；④ 拉起——`AIRfcTaskGraph.AttachItem`（运行中增项 + 重复 Id/未知依赖拒绝）+ `AIParallelCoordinator.SpawnAsync`（派发原语：在飞数校验 + 惰性取租约）+ `SetParallelism`（弹性并行度）。
- **组件协作**：`AIParallelCoordinator`（reconcile 循环 + 撤单收束 + 决策广播/定向 + 旁路注入）· `AIRfcTaskGraph`（静态拓扑 + MarkCancelled）· `AISubAgentRun`（RfcRevision 快照 + `Spawn/Interrupt/Reap/CheckpointInterrupt/ResumeAfterSync/Realign/RevalidateLease` + `PendingMessages` 邮箱）· `AISubAgentState`（生命周期状态机）· `AISubAgentMessage` / `AISubAgentDecision`（§8 载荷）· `AISubAgentLeaseGate.Revalidate`（租约重验）· `AIHarnessSession.ReviseRfc` + `RfcRevisionChanged`（广播钩子）· `AIRfcRuntime.Revise`（RfcSpec 租约；**`Revise:90` `next.WorkItems = current.WorkItems` 共享列表引用破坏旧版只读审计——别名 bug**）· `AIDoDOrchestrator.RunAggregatedGatesAsync`（要求全部 Completed + HasSummary）· **REPL 零接线**（无 `/parallel` `/cancel` `/sync` 命令，`AIParallelCoordinator` 仅被 e2e 消费；`RfcRevisionChanged` 钩子已存在，由持有管理器实例的宿主接线）。
- **能力面**：并行容器 / 升版闭环 / 冲突织物为真底座；**撤单收束**（`CancelPendingAsync` 取消未启动 + 中断在飞 + 即时释放租约 + 修取消被吞）；**决策同步**（`PendingSyncDecisionAsync` 广播 / `SyncDecisionAsync` 定向 / `EnqueueMessageAsync` 旁路注入 / revision-changed 重对齐 + 租约重验 / work-item-rescope / wrap-up / `subagent:sync` 事件）。
- **诚实缺口**：**多事件仲裁（S-6）仍体系空白**（动态派发已落地、成本核算已落地，见 [subagent-management §9](subagent-management.md)），REPL 无并行管理器实例接线。
- **契约对照**：[parallel-subagents](parallel-subagents.md) §4.2 只定义「启动时注入」、§3.4「启动时只读快照」隐含「纠偏后可回收重派」，实现无广播 / 收束 / 取消；「`TotalBudget` 超限强制收束」为设计态；A.9 已登记「无 CancelPending 广播」。
- **能力面（补件）**：① 停止：`CancelPendingAsync` / `InterruptAsync` + 修 `Complete()` 覆盖取消 + 失败/死亡即时 `ReleaseSession` + `AIRfcWorkItemStatus.Cancelled`；② 同步：`SyncDecisionAsync`（广播 / 定向二分 + soft / hard 收束）+ `revision-changed` 订阅 + 租约重验——`PendingSyncDecisionAsync` 广播 / `SyncDecisionAsync` 定向 / `EnqueueMessageAsync` 旁路注入 / revision-changed 重对齐 + `RevalidateLease` 租约重验 / work-item-rescope / wrap-up / `subagent:sync` 事件；③ 拉起：`AIRfcTaskGraph.AttachItem` + `SpawnAsync`（并行度余量校验 + 惰性取租约，A4 ⬜）；④ 多事件合并为**单次干预批次**（Stop → Sync → Spawn）+ `airfc:interrupt / sync / spawn` 事件 kind 入决策轨迹（单轨；`subagent:interrupt` / `airfc:cancelled` / `subagent:sync` 已入）；⑤ REPL 接线（`/parallel status` / `/cancel` / `/sync`；`RfcRevisionChanged` 钩子已备）；⑥ 修 `Revise` WorkItems 别名。全部归入 §6 阶段 D（方案 A 演进 A1–A5，见 [subagent-management §9](subagent-management.md)）。

### 4.1 验收功能对不上

- **用户动作**：交付后验收发现功能与需求对不上。
- **运转流程**：`/dod` → D3 跑 `arc test <proj> --logger json` → **用例级明细解析**（`D3TestReport`：passed>0 且 failed==0 才绿；防降级基线——用例数骤降判红；结构化 Acceptance `TestName` 对照）→ D5 证明槽位（**机器校验**：`D5ProofVerifier` 验证证明引用存在性——文件存在 / 测试名 `arc test --list-tests` 可解析；无校验/无效标红而非 Passed）→ D7 人验收；「功能对不上」经「验收条目 ↔ 测试 ↔ 实际运行结果」结构化对照捕捉，不再只靠人眼。
- **组件协作**：Acceptance 结构化 `AIAcceptanceSpec.Items`（`AIAcceptanceItem`：场景+断言+验证命令/测试名；`/revise --acceptance= --test= [--verify=]` 落结构化）· D5 `D5SelfReview.SetProof` + `D5ProofVerifier`（Coding 机器校验）· D3 `CodingDoDGateEvaluator.EvaluateD3Async` + `D3TestReport`（`--logger json` 明细 + 防降级）· D7 `CompletePlanAfterDoDAsync`。
- **能力面**：原 D 断点（D5 证明无机器校验、D3 只退出码）已消除：证明引用存在性机器判定（文件/`--list-tests` 测试名可解析）、D3 用例级明细（断言绿非退出码绿）+ 防降级（用例数骤降 `test count reduced` 判红）+ Acceptance TestName 对照（幽灵测试红）；原 E 断点（Acceptance 纯文本）已消除：结构化条目面 + `ToContextBlock` 折叠 `acceptance-items`（D5 真实测试 Valid / 幽灵 Invalid / 文件 Valid / 空 Missing；D3 passed=2 绿 / TestName 幽灵红 / 测试改弱用例数骤降红）。
- **契约对照**：D5「每条 acceptance 附可执行证明」、D3「断言独立于实现 + 结构化断言 diff」（[definition-of-done](definition-of-done.md)）；实现已从文本槽位 + 退出码升级为机器校验证明 + 用例级明细。
- **诚实缺口**：D5 深度校验（证明「真实运行通过」而非仅引用存在）、D3 断言级 diff 解析（B6/A.7）、防降级基线跨会话持久化（阶段 F）。

### 4.2 UI 效果差距

- **用户动作**：交付 UI 后用户发现视觉效果 / 交互与预期差距。
- **运转流程**：**无此流程**——D0–D7 无任何 UI 视觉 / 交互门（D0 编译 · D1 语义 · D2 契约 · D3 行为测试 · D4 diff · D5 自审 · D6 反模式 · D7 人验收，无渲染 / 交互断言面）。
- **组件协作**：无——arc-ui / ARML 渲染管线不在 Coding DoD 信号内。
- **诚实缺口**：**体系空白**——D0–D7 无 UI 视觉 / 交互门，UI 效果差距无机器 / 半机器验收路径。
- **契约对照**：043 宣称 Harness 交付「可感知结果」；对 UI 交付无可感知结果的判定门。
- **能力面（补件）**：UI 门——渲染快照 / 黄金文件、交互断言、视觉回归；利用 arc-ui 渲染管线产出可判定信号（§6 阶段 E）。

### 4.3 测试不通过

- **用户动作**：交付时验收测试不通过。
- **运转流程**：`/dod` → D3 跑 `arc test <proj> --logger json`（**真实 flag**，`arc test --help` 列出 `json|junit`）→ 红 → **L2 自动迭代**（`RunFixLoopAsync`：回喂 → `RecordFixAttempt` ≤3 轮 → 重跑门）→ 收敛或超限回滚升级人。
- **组件协作**：D3 信号 `QualityCli`（`--logger json` 真实支持）· `AIDoDOrchestrator.RunFixLoopAsync`（迭代驱动，`RecordFixAttempt` 消费）· `AIHarnessSession.RunFixLoopAsync`（超限回滚 + 升级人）。
- **能力面**：D3 `--logger json` 为真；**含 `Main` 的 app 项目可正常 `arc test`**（测试模式 `strip_entry_main` 剔除用户顶层 `Main`、合成 `__QifTestHost::Main` 接管入口）；**`RecordFixAttempt` 已消费**（L2 自动迭代接线，与 2.3 同根）——测试失败后自动迭代闭环成立。
- **契约对照**：[definition-of-done](definition-of-done.md)「L2 自动判定：测试失败 → 结构化断言 diff 喂回迭代 ≤3 轮」；实现迭代驱动已接线，回喂载体为 `--logger json` 原文文本（断言级 diff 解析挂账，B6/A.7 同源）。
- **能力面（补件）**：① L2 迭代接线（同 2.3）；**缺口**：D3 断言级 diff 解析（B6）。

---

## §4 治理 / 质量 / 方法论场景（A/B 组）

> 两组由**行业研发体系方法论**（需求工程 IEEE 830 / RTM · 敏捷 / 瀑布 · 变更管理 CCB · 测试金字塔 · 配置管理 / 基线 · 技术债治理 · 治理与合规）对照当前 Harness 能力推导的**只读结论**（A 组 10 · B 组 12），按与原有 14 场景相同的六段式补全。**含「待验证」部件不得宣称能力已具备（§0 门闩 / §7 标注）**。A/B 组各场景的能力面与诚实缺口见各场景对断面。**B 组另加并入 B3 的「git 分支迭代与合并」深度子面（B3′，见 §B3′）**——分支模型 / 合并门 / 冲突裁决 / 合并绿点四项组件部分仍为设计态，机制层设计见 [conflict-branch](conflict-branch.md)。

### 4.0 与原场景体系的合并映射

**并入增强**（同门类增强，不新开门类）：

| 场景 | 并入 | 说明 |
|------|------|------|
| A.2 需求冻结 CCB | 3.3 / 2.2 | 3.3「半途改需求」的**锁定控制上层**（Freeze 态）；2.2「门状态面」补 Freeze 态。同属「需求变更管理」门类 |
| A.3 范围蔓延 | 1.3 / 2.2 | 1.3「多轮需求讨论」范围度量；2.2「`/status`」指标面；D4 越界检测扩展为「实现+需求」双信号 |
| A.5 依赖升级 | 2.1 | 2.1「现有项目」D1 影响分析 + D3 子图回归选择（SR-2） |
| A.7 NFR 进验收 | 4.1 | 4.1「验收对照」机制扩展 + 新增 D9/D10 门（同属验收面扩展） |
| A.8 验收环境数据 | 4.3 | 4.3「测试不通过」的**前置场景**（环境不可用 vs 测试红，同属验证面） |
| A.9 撤单 Cancel | 3.1 / 3.4 | 3.1/3.4「纠偏 / 推倒重来」的**收束语义补全**（Reject→Cancel 状态 + 在飞收束），同属执行收束门类 |
| 运行中子代理干预（3.5 细分） | 3.1 / 3.3 / A.9 | S-1 补充问题 / S-2 纠正需求 / S-4 决策同步 → **3.1**（执行中纠偏 · 在飞通信子面）；S-5 拉起新子代理 → **3.3**（半途增需求 · 动态增项）；S-3 停止子代理 → **A.9**（并入 3.1/3.4 收束门类）。同属「并行执行干预」门类；S-6 多事件混合开新门类 **3.5** |
| git 分支迭代与合并（B3′） | B3 / A.6 / 3.2 | 分支模型 + 合并门（汇总门 + CI + 人评审）+ 合并时冲突裁决 → **B3** 主分支保护增强；分支隔离 + 基线所有权 → **A.6** 子面；`AIMergeTransaction` git 版两阶段提交 → **3.2** 同根（但语义正交：ToolPath 租约 = 单进程写时互斥，分支冲突 = 跨分支合并时裁决） |

**新门类**（原 14 场景无此面向，须新开）：

| 门类 | 场景 |
|------|------|
| 多来源需求裁决 | A.1（原体系只处理单人会话内需求演进；多来源/矛盾需求的裁决是项目管理面，且依赖 A.6 的多用户基础） |
| 遗留项目接入 | A.4（原体系全部从需求出发做新东西；无任何资产——无 `.arcgr`/无测试/无 Wiki——的遗留项目是另一量级，2.1 只覆盖有 Wiki/约定的现有项目） |
| 多用户仓库协作 | A.6（跨进程织物 / 分支模型 / 基线所有权裁决，与 P3 单宿主并行正交） |
| 过程改进复盘 | A.10（底座已有，`/retro` 聚合可先并入 2.4，完整门类仍为新） |
| 运行中子代理干预协议（3.5） | S-6 多事件混合：停止 / 同步 / 拉起的**仲裁顺序与批次原子性**（执行域新协议层，与 3.1–3.4 并列；6 细分中 S-1/S-3/S-4/S-5/S-6 并入门类缺口，见 §3.5） |
| 合并时冲突裁决（B3 门类下新子门类） | 跨分支双改的「合并时裁决」——与 ToolPath 租约正交（N4 禁自动选胜者落地），见 §B3′ 卡 4 |
| B 组 12 场景 | B1 CI 集成 · B2 回归策略 · B3 主分支保护 · B4 发布/里程碑 · B5 事故 Hotfix · B6 测试质量 · B7 性能可观测 · B8 安全合规 · B9 多项目工作区 · B10 团队扩展 · B11 AI 降级 · B12 知识沉淀（执行/质量/治理方法论域，原 14 场景未触碰） |

### 4.1 A 组场景推导（A.1–A.10 · 需求/变更/协作域）

> 判定基准与 §7 同口径：真（代码 / e2e / 文档登记佐证）· 空想（文档宣称、实现零消费 / 设计态）· 待验证（已修未现场复验）。

#### A.1 需求来源冲突 · 多方向并存（新门类「多来源需求裁决」）

- **用户动作**：多人在同一 AIRfc 上各加方向：A 会话「做 X」，B 会话（顺序或并发）「不要 X 要 Y」；需求出现多个相互矛盾来源。
- **运转流程**：① 并发冲突**真拦截**——Session A/B 同时 Revise → 均持 `AILeaseKind.RfcSpec` 租约 → `AICoordinator` 单表授予先到、后到 `Acquired=false` 拒绝；② **顺序矛盾判定**——A 升版 Revision N→N+1 释放租约后，B 的 Revise 经 `AISpecConflictDetector` 字段级结构化 diff（`AIAcceptanceSpec.Items` 条目级比对：异来源覆盖同 acceptance 项 → 反方向覆盖信号）→ 不落新 Revision，标 `AIRfcStatus.Contested` + 登记 `AIConflictRecord`（Kind=SpecContradiction / Resources / Parties[来源A,来源B] / Evidence=diff）+ `conflict:detected` 决策事件；③ **人 CCB 裁决通道**——`/conflict` 列出 Open 冲突（方向/owner/evidence）+ `AIConflictResolver.ResolveAsync(conflictId, decision, reason, resolvedBy)`（**resolvedBy 必须显式人，空 → false，机器不可自动选胜者**）→ 裁决后 Contested → Active **新 Revision 基线** + `conflict:resolved` / `airfc:resolved` 决策事件；`RejectAsync` → 记录 Rejected + AIRfc → Rejected。
- **组件协作**：Session A/B ─`Revise(RfcSpec 租约)`→ `AICoordinator`（先到授予 / 后到拒绝，真）→ `AIHarnessSession.ReviseRfc`（L2 预检：`AISpecConflictDetector.Detect` → `MarkContested` + `AIConflictResolver.RecordSpecContradiction` + `conflict:detected`）→ `AIConflictResolver.ResolveAsync`（人 CCB）→ `AIRfcRuntime.ResolveContestedWithSpec`（新 Revision 基线）→ `airfc:resolved` 事件。
- **能力面**：并发租约拦截（真）+ 顺序矛盾判定 + 人 CCB 裁决 + 冲突全程入决策轨迹（`conflict:*` / `airfc:resolved`）——判别性验收（两方顺序反方向覆盖 → Contested + 记录 Open；机器空 `resolvedBy` 不可裁决；人 Resolve → 新基线 + airfc:resolved；Reject → Rejected）。
- **诚实缺口**：Owner/Priority 字段（RTM 完整追溯，A.1 补件③）；`/conflict` REPL 为库级 API 直测等价（REPL 交互命令已接线，自动化交互测试依赖控制台）。
- **契约对照**：冲突织物宣称「多任务安全完成」仅覆盖写冲突；parallel-subagents N4「禁自动裁决合并冲突 → 整体回滚 + 升级人」是 ToolPath/合并事务语义，未延伸到 Spec 方向冲突；api-sketch §2 AIRfc 字段级契约无 Source/Owner/Priority——`AIRfc.Source` 为来源追踪字段。
- **能力面（补件）**：① `/revise` 事件带 Spec 字段级结构化 diff（旧版 vs 新版 Acceptance 条目）→ 机器可判互斥（覆盖同一 acceptance 项）→ 标 `AIRfcStatus.Contested`；② 新命令 `/conflict <rfcId>`：列出并行方向/owner/未决冲突 → 人（CCB）裁决 → `airfc:resolved` 事件写新 Revision 基线；③ Revision 事件带 Source/Owner/Priority（RTM 可追溯）——Source 已落，Owner/Priority ⬜（挂账）。

#### A.2 需求中途冻结 · 冻结窗口（⚠️ 部分 · 并入增强 → 3.3/2.2）

- **用户动作**：临近验收/发布要锁需求；冻结窗口内只允许 CCB 批准的变更。
- **运转流程**：① 基线骨架真——Revision 单调递增、旧版 Superseded 只读、Rejected→Active 非法（airfc §4.2 边表）；② **状态枚举**——`AIRfcStatus` 增 `Contested`/`Frozen`/`Closed`/`Cancelled`，运行时 `FreezeRfc`/`UnfreezeRfc`/`CloseRfc`/`CancelRfc` + `Frozen` 禁 revise / `Closed` 禁再改；但无「冻结窗口」上层治理（缺口）——无 `/freeze` REPL、无 CCB 裁决、无「冻结期变更须先影响分析」门；③ 发布后收口有运行时入口——D7 通过后 `CloseRfc` 可置 `Closed` 终态，但 `/dod` 通过不自动触发 Close，`AIPlan.Completed` 与 AIRfc `Closed` 无联动；④ 无 AICR（Change Request）类型、无 CCB 裁决（缺口）。
- **组件协作**：`/revise` → `AIRfcRuntime.Revise`（Revision+1 · 旧版 Superseded 只读，真）；冻结期无 Freeze 状态机 → 直接放行（缺口）；发布后 `AIPlan.Completed` 而 AIRfc 仍 Active（缺口）。
- **能力面**：有 Revision/Superseded 基线骨架 + `Frozen`/`Closed`/`Cancelled` 状态已入枚举（`FreezeRfc`/`CloseRfc`/`CancelRfc` 运行时）。
- **诚实缺口**：缺 CCB 裁决（AICR）、影响分析门、`/freeze` REPL 命令。
- **契约对照**：AIRfc 定位「跨任务、跨版本唯一事实源」——生命周期状态枚举已扩展（`Contested`/`Frozen`/`Closed`/`Cancelled` 已入），但「冻结窗口 CCB 治理」（AICR / 影响分析 / `/freeze` 命令）仍缺；3.3 只覆盖「半途改需求」的升版，无「锁定后变更走 CCB」的上层控制。
- **能力面（补件）**：① `AIRfcStatus.Frozen` 入枚举 + `/freeze <rfcId> [--reason]` REPL 命令（冻结后 `/revise` 拒绝并返回原因）；② 新类型 `AICR`（Change Request：描述/影响面/发起人/紧急度）+ `airfc:ccb_approved` 事件：冻结期变更走 CCB 人裁决，通过才放行 Revision+1；③ 影响分析钩子：冻结期变更须列出受影响门/测试/工作项并重跑（与 A.5 共用）；④ `AIRfcStatus.Closed` 入枚举 + D7 通过后自动 `CloseRfc`（对标 Release 后需求锁定）。

#### A.3 范围蔓延 Scope Creep（⚠️ 部分 · 并入增强 → 1.3/2.2）

- **用户动作**：每次 `/revise` 都「顺便加一点」，需求静默膨胀；期望预警与收敛机制。
- **运转流程**：① 增补自由真——`/revise --intention=/--design=/--acceptance=` 任意增补，Revision+1；② 无范围度量——不记录每版 acceptance 条目数 / plan step 数 / scope 声明文件数；无「自 Revision 1 累计增长」指标（缺口）；③ 最近的反蔓延真机制 = D4 diff 越界检测（改动超出 AIRfc/计划边界 → 过度设计信号 → 升级人，`D4DiffCoverage` 真）——只管「实现越界」，不管「需求增长」；④ `AIDesignSpec.Convergence` 是文本字段，无机器度量。
- **组件协作**：`/revise` → `AIRfcRuntime.Revise`（Revision+1，无 scope_delta 事件 / 无度量，缺口）；实现改动 → `D4DiffCoverage` 越界检测 → 升级人（真；只管实现不管需求）。
- **能力面**：有 D4 越界单点信号。
- **诚实缺口**：缺跨 Revision 范围度量、蔓延阈值门、NonGoals 边界面。
- **契约对照**：`AIDesignSpec.Convergence` 字段存在但纯文本；1.1/1.3 已列「acceptance 纯文本无自动锁测」——范围蔓延与它同根于 Spec 未结构化。
- **能力面（补件）**：① `/revise` 时记 `scope_delta` 决策事件（acceptance 条目数 / plan steps / scope 文件数 / 预估成本增量）+ `/scope <rfcId>` 报告自基线累计增长；② 蔓延阈值门：累计增长超阈值（如 acceptance +50%）→ `airfc:scope_warning` 升级人（与 A.2 CCB 共用裁决）；③ AIRfc 增 NonGoals 面（不做清单）；新增 acceptance 与 NonGoals 冲突 → 拒绝/告警；④ DoR 硬门（与 1.1 补件①同）：acceptance 未结构化禁止 `/revise` 增补。

#### A.4 技术债与遗留代码 · 接手历史项目建基线（⚠️ 部分 · 新门类「遗留项目接入」）

- **用户动作**：接手无 `.arcgr`、无测试、无 Wiki、甚至当前不可编译的历史项目；先建基线再动刀。
- **运转流程**：① 上下文注入真——`ProjectConventionsProvider` 读 `.arcagent/conventions.md`（缺失 → 无贡献）、`AIWiki`（缺失 → 无注入）；② `/rfc` 立项 → `/dod`：D0 `arc build`（退出码）；D1 `.arcgr` → 项目级 K1/K2/K3（顶层 Main 收录 / 跨文件符号+New/Call edges / MethodCall 边）→ D1 基线数据可生成；③ 无 `arc baseline` / `/inherit` 流程：不生成 `.arcgr`、不补 characterization 测试、不登记技术债基线（待补）；④ 真部件：`D6AntiPatternScan`（TODO/NotImplemented/todo!() 判红）可作债清单信号源；`CheckpointGreenAsync` 快照可作基线快照（多绿点历史 `index.json` + checkpoint-<seq>.json，3.4 已闭环），但要求 D0–D3 绿（遗留项目通常过不了），半真；⑤ 无「债登记 → AIPlan 修债步骤」追溯面（待补）。
- **组件协作**：`ProjectConventionsProvider` / `AIWiki` ─注入→ Rules 层（真）；`/rfc` → `/dod` → D1(`.arcgr`) → K1–K3 已修 → 基线数据可生成；`D6AntiPatternScan` ─TODO/占位计数→ 债信号（真）；`CheckpointGreenAsync` ─`index.json`+checkpoint-<seq>→ 基线快照（多绿点，3.4 已闭环；需 D0–D3 绿，遗留项目不可用，半）。
- **能力面**：有 D6 债扫描 + 约定/Wiki 注入 + 绿点快照。
- **诚实缺口**：缺建基线流程面：`arc baseline`、characterization 测试、`.arcgr` 生成、债登记。
- **契约对照**：2.1 已证「`.arcgr` 项目级 K1–K3 已清 → D1 引用图有数据」；043 背景「随生态分发、开箱即用」对无任何资产的遗留项目无建基线路径（1.2 空目录脚手架已闭环，但遗留项目接入的 `arc baseline` 面仍空白）。
- **能力面（补件）**：① `arc baseline <dir>` / `/inherit`：自动跑 D0+D6 静态扫描生成「接手基线报告」（可编译? / 反模式计数 / 契约违约计数）；无 `.arcgr` → 触发生成或 D1 诚实降级 Pending + 登记债；② Characterization 测试生成（golden-master）：对无测试代码生成「当前行为 = 期望」护栏测试，绿后作 D3 基底线（测试金字塔补底层）；③ `AITechDebt` 登记类型（类别/严重度/位置）或复用 D6 扫描 → `/debt` 列表；修债作 AIPlan 工作项（债→计划可追溯）；④ 债预算：接手时登记债基线，「只增不减」超限预警（技术债治理度量面）。

#### A.5 依赖库/框架升级 · 跨切面变更（⚠️ 部分 · 并入增强 → 2.1）

- **用户动作**：升级 std 依赖 / 改 ABI / 升级框架；跨切面影响多包，要规划、验证、可回滚。
- **运转流程**：① 人门真——协作确认点「改 public API/ABI/std → 必须确认（影响面 X 包）」（`CollaborationCheckpoints.DetectAsync` + D7 必须确认）；② 影响分析半——D1「契约影响：改 public API → 下游使用点已验证」依赖 `.arcgr` 引用图 → K2/K3 已清（跨文件符号+New/Call edges + MethodCall 边）→ 影响面机器算有方法边数据（残留：K5 增量粒度）；③ 回归面半——D3 `arc test`（含 Main 冲突已修）+ 增量选择靠 `--incremental-report` 文件级（K5）→ 升级后受影响子图回归不可行（缺口）；④ 回滚半——绿点多版本回滚为真（3.4 已闭环：`index.json` + checkpoint-<seq> + 大文件对象副本 + 联动 AIRfc/Plan），但跨会话 AIRfc/AIPlan/门状态不持久（2.4）→ 升级失败回滚可用；⑤ 无 SemVer/ABI 兼容校验、无升级向导、无 changelog 事件（待补）。
- **组件协作**：升级意图 → `CollaborationCheckpoints`（public API/ABI 必须人确认，真）→ D1(`.arcgr` 影响分析) → K2/K3 已清 → 影响面机器算有方法边数据 → D3(`arc test`) → 含 Main 测试已修 + K5 文件级增量 → 回归面仍不足（缺口）→ 失败 → `CheckpointRollbackAsync(checkpointId)` → 回滚（真，多绿点可指定，3.4 已闭环）。
- **能力面**：有 ABI 人确认点 + 绿点回滚。
- **诚实缺口**：缺依赖升级规划、机器影响分析、SemVer/ABI 校验、子图回归选择。
- **契约对照**：definition-of-done D1 行宣称「改 public API → 下游使用点已验证」，实现 `.arcgr` 项目级 K1–K3 已清（引用图有数据）；「增量验证受影响子图」（SR-2）未排期（K5）。
- **能力面（补件）**：① D1 真项目可用（K1–K3 已清）→ 升级自动影响分析（引用图：升级库 → 受影响包/符号/使用点清单）——升级验证根基；② AIPlan 升级模板 / `/upgrade`：步骤 + 兼容性检查点（SemVer 主版本/ABI 符号 diff → 强制 D7 确认 + 影响面报告）；③ SR-2 增量验证落地 → D3 只跑受影响包；④ 升级前强制绿点 + 记 AIPlan 基线，失败回滚联动 AIRfc 事件（复用 3.4 补件）；⑤ `airfc:upgrade` 事件 + 自动升级小结（SemVer 类别 → changelog）。

#### A.6 多人/多 Agent 同仓库协作（⚠️ 部分 · 新门类「多用户仓库协作」）

- **用户动作**：多用户/多 Agent 在同一仓库干活：分支模型、合并冲突、谁拥有基线的裁决。
- **运转流程**：① 单宿主内真——P3 `AIParallelCoordinator` + `AICoordinator` 租约（ToolPath 后到拒绝）→ 同 host 内多子代理写面互斥；`independent_scope` / `toolpath_conflict` 用例在冲突织物修复后转绿；② 跨进程/跨用户空——两个独立 arc 进程 = 两个独立 `AICoordinator` 登记表，租约不共享 → 无跨进程互斥，唯一仲裁者是 git（缺口）；③ 基线裁决半——**SR-1**：`AIPlan.Id` 稳定租约键（`"plan:"+Id`，跨会话）；仍无「谁拥有基线」规则、无分支感知、无冲突基线裁决（跨进程面缺口）；④ 合并验收半——P3 汇总门 `RunAggregatedGatesAsync` 对合并后总工作区跑完整 D0–D7（唯一权威，真）；`AIMergeTransaction` 两阶段提交为设计态 → 合并冲突处置靠 git + 人（缺口）。
- **组件协作**：同宿主：子代理 ─`ToolPath` 租约→ `AICoordinator`（单表）→ 后到拒绝（真）；跨进程：进程1/进程2 ─各自登记表→ 无共享 → 无互斥（缺口）；合并：`RunAggregatedGatesAsync`（合并后总工作区 D0–D7，真）← `AIMergeTransaction`（staging→Move，设计态）；基线：`AIPlanGate.AcquirePlanLease`（Goal 合成键，不稳；SR-1 待闭环）。
- **能力面**：单宿主内冲突织物真。
- **诚实缺口**：缺跨进程共享织物、基线所有权裁决、分支隔离；多用户子面偏空。
- **契约对照**：conflict-fabric 宣称「跨会话、多任务并行」——实现为进程内登记表，跨进程不共享；SR-1（`AIPlan.Id`）为稳定租约键；「禁自动裁决合并冲突」（P3 N4）原则的 `AIMergeTransaction` 为设计态。
- **能力面（补件）**：① 跨进程共享冲突织物：租约持久化到共享面（repo 级 `.arcagent/leases/` 或 server 面）——架构级决策须 RFC；② SR-1 `AIPlan.Id` 稳定租约键（跨会话）；③ 分支模型：`/branch` 感知 + 绿点/门状态按分支隔离（`arc-checkpoints/<branch>/`）+ 基线所有者声明（AIRfc/AIPlan 绑定分支+owner）；④ 合并冲突 → 升级人裁决（N4 落地）+ `AIMergeTransaction`（staging 全落 → 统一 Move → 冲突整体回滚）；⑤ 合并后验收复用汇总门（已存在）作为合并交接的唯一验收权威。

#### A.7 非功能需求进 Acceptance 与 D3（❌ 空想 · 并入增强 → 4.1）

- **用户动作**：把 NFR 写进 Acceptance，并由 D3 级机制验证：基准测试门、安全门、可访问性门。
- **运转流程**：① Acceptance 面纯文本（`AIAcceptanceSpec.Scenarios/Assertions` 为 string）→ 无法表达「量化指标 + 阈值」NFR 条目（缺口，1.3 同根）；② D3 只退出码（`QualityCli.IsGreen` exit=0；含 Main 冲突已修）→ 无基准测试信号、无性能回归（缺口）；③ 安全：AG-4 fail-open 已修（真）；协作确认点覆盖 API/ABI 面（真）；但无安全扫描门（密钥/危险调用/依赖漏洞）（缺口）；④ 可访问性：D0–D7 无 UI 门（4.2 体系空白）→ 可访问性无门（缺口）；⑤ 门扩展机制真：`AIDoDGateKind` 枚举 + `IAIDoDGateEvaluator` 策略注入可挂新门。
- **组件协作**：`AIAcceptanceSpec` ─纯文本→ 无 NFR 结构化（缺口）；D3 ─`arc test` 退出码→ 无基准/性能面（缺口）；`AIDoDGateKind`(D0–D7) + `IAIDoDGateEvaluator` ─扩展点→ 可挂 D9/D10 新门（真机制）；D6 反模式扫描 ─与安全扫描同构→ 可复用（真）。
- **诚实缺口**：**体系空白**——NFR 验证面：无基准测试、无安全扫描门、无可访问性门、Acceptance 无量化面。
- **性能子面设计态**：性能 / 日志筛选面已有具体设计落盘——[performance-observability](performance-observability.md)（`AIPerfMonitor` / 性能信号模型 / 异常分类表 / **增强信号 → D9 门**）与 [signal-log](signal-log.md)（`AISignalLog` 分级筛选 / `AIToolOutput` 工具输出门面），演进 P1–P3（plan `PF-1–PF-3`）；**D9 门仍设计态**——安全 / 可访问性子面仍空想，判定维持 ❌ 直至 D9 门落地并经五面推演验收。
- **契约对照**：D3 文档宣称「验收测试全绿；断言独立于实现」，实现只退出码（含 Main 冲突已修后 app 项目可测）；「防降级」无性能维度；4.2 已证 UI 视觉门体系空白；P3 `TotalBudget` 是子代理成本预算，非产品性能预算（parallel-subagents §4.5）。
- **能力面（补件）**：① `AIDoDGateKind` 扩展 D9 性能门（`arc bench <proj>` → 基线 diff 回归阈值；**设计态** [performance-observability](performance-observability.md) D9）+ D10 安全门（密钥/危险调用/依赖漏洞扫描，与 D6 同构）；② `AIAcceptanceSpec` 增结构化 NFR 面（指标 + 阈值 + 量化单位，非纯文本；与 1.3 补件①合并）；③ 性能基线存储并入绿点体系（性能绿点随 checkpoint 落盘，回归 → 回滚或升级人）；④ 可访问性并入 4.2 UI 门（渲染快照 + 交互断言 + 可访问性规则检查）。

#### A.8 验收环境/数据问题（⚠️ 部分 · 并入增强 → 4.3）

- **用户动作**：验收跑不起来或不可信：缺 API key、依赖外部服务、测试数据不真实。
- **运转流程**：① 升级规则真——L2 边界表「验证环境不可用（缺 `ARC_DEEPSEEK_API_KEY` / 平台依赖缺失）→ 升级人，不重试烧钱」（definition-of-done L2 升级人表）；e2e 无 clang/arc 时 skip（`if !clang_available() { return; }`）真；② 无环境就绪检测门——无「验证所需环境清单 → 缺失即标 NeedsHuman」的机器前置检查；升级人是 prompt 文本级规则，非门（缺口）；③ Acceptance 无环境依赖标注（`[Requires: X]` / `[Mockable]`）→ D3 无法分级（本地 mock vs 真服务）（缺口）；④ 测试数据：无 fixture 约定、无种子命令、无数据真实性/掩码校验（缺口）；⑤ D5 证明字符串无法引用环境事实（无机器校验，SR-3 相关）（缺口）。
- **组件协作**：L2 升级人：验证环境不可用 → 升级人（不重试烧钱，真，文档+行为）；e2e：`!clang_available()` → skip（真）；`/env` 前置检测 / D3 分级（mock·真服务）/ 测试数据管理（均缺口）。
- **能力面**：有「环境不可用 → 升级人」规则与 e2e skip 行为。
- **诚实缺口**：缺环境就绪检测门、mock/契约测试约定、测试数据管理。
- **契约对照**：L2 表把「验证环境不可用」列为升级场景（真实现义）；但无环境清单/检测类型；4.3 只覆盖「测试不通过」，未覆盖「测试根本跑不起来」。
- **能力面（补件）**：① 环境就绪检查（DoR 前置）：`/env` 或 D3 前置信号——验证环境依赖清单化（key/工具/服务/数据），缺失 → 标 NeedsHuman 且不触发测试（与 L2「不重试烧钱」一致）；② Acceptance 条目环境标注：`[Requires: service]` / `[Mockable]` → D3 分级运行（mock 面本地、真依赖面留人/CI）；③ 外部服务用契约测试（consumer-driven contract）约定进 testing-first，真实服务仅必要面触发；④ 测试数据管理：fixtures 目录约定 + 种子命令（如 `arc test --seed`）+ 数据真实性/掩码校验。

#### A.9 人为撤单 / 用户放弃任务 Cancel（⚠️ 部分 · 并入增强 → 3.1/3.4）

- **用户动作**：用户中途撤单/放弃任务。AIRfc、工作项、已写代码、绿点、轨迹如何处置。
- **运转流程**：① Reject 真——`RejectRfc`（Active→Rejected + `airfc:rejected` 事件）；Rejected→Active 非法（须新 Revision）——覆盖「方向被否」，不覆盖「放弃」（半真）；② **撤单语义**——`AIRfcStatus.Cancelled`（`AIRfcRuntime.CancelRfc` Active/Frozen/Rejected→Cancelled）+ `AIRfcWorkItemStatus.Cancelled`（Open/Blocked/InProgress 置 Cancelled，不进下一波）+ `airfc:cancelled` 事件；`/cancel` REPL 命令仍缺（库级可调，接线待 REPL）；③ 已写代码处置无选项——keep-wip/rollback 处置协议未接线；`/rollback` 真实可回滚到指定绿点（多绿点历史 + 联动 AIRfc/Plan，3.4 已闭环）（缺口）；④ **在飞收束**——`AIParallelCoordinator.CancelPendingAsync(rfcId)`：取消未启动 + 中断在飞（联动 CTS + `AITaskRun.Cancel`，`subagent:interrupt` 事件）+ 即时释放该 Rfc 全部租约（路径可被后续取得）+ 回收容器（Dead）；**取消被吞已修**（`RunAllAsync` 不再无条件 `Complete()` 覆盖，ct 取消/撤单 → 子代理 Cancelled + 必答小结，汇总门按未完结红）；⑤ 轨迹：`airfc:cancelled` / `subagent:interrupt` 事件 kind 已入（单轨）；reopen（`airfc:reopened`）未做。
- **组件协作**：`/reject` → `AIRfcRuntime.RejectRfc` → Rejected + `airfc:rejected`（真；仅方向被否）；`CancelRfc` → `AIRfcStatus.Cancelled` + `airfc:cancelled`（真）；`AIParallelCoordinator.CancelPendingAsync` → 在飞中断 + 未启动取消 + 即时释放租约（真）；`/rollback` → `CheckpointRollbackAsync(checkpointId)`（真；多绿点可指定 + 联动 AIRfc/Plan，3.4 已闭环）；`/cancel` REPL 命令 + keep-wip/rollback 处置选项接线（缺口）。
- **能力面**：撤单语义 + 在飞收束 + 取消被吞修复。
- **诚实缺口**：**缺 `/cancel` REPL 接线、代码处置选项（keep-wip / rollback）、`airfc:reopened` 续跑路径**。
- **契约对照**：airfc §4.2 原仅定义 Active/Superseded/Rejected 迁移边；已增 Active/Frozen/Rejected → Cancelled 边；P3 `AISubAgentRun.RfcRevision` 只读快照隐含「可回收重派」——以 CancelPendingAsync 落「取消未启动 + 中断在飞 + 撤销已占租约」；「绿点进决策轨迹 → 可 /resume」无「取消后继续」路径（2.4，reopen 待排期）。
- **能力面（补件）**：① `AIRfcStatus.Cancelled` + `AIRfcWorkItemStatus.Cancelled` + `airfc:cancelled` / `subagent:interrupt` 事件 kind；② `AIParallelCoordinator.CancelPendingAsync(rfcId)`：取消未启动 + 收束在飞 + 即时释放租约；③ 修取消被吞（`Complete()` 不再无条件覆盖，ct 取消 → Cancelled + 小结，汇总门红）。**挂账**：`/cancel <rfcId> [--keep-wip | --rollback]` REPL 接线；处置协议（keep-wip 标 Cancelled 但 WIP 保留 + `airfc:reopened` 新 Revision 可续 / rollback 回滚最近绿点 + `checkpoint:rollback` 事件）；撤单/reopen 事件后续扩展。

#### A.10 复盘与回顾 Retrospective（⚠️ 部分 · 新门类「过程改进复盘」）

- **用户动作**：任务完成后总结：哪些流程有效/无效、改进项；复盘应否进体系。
- **运转流程**：① 底座真——`AIWorkSummary` 五字段（做了什么/对齐/验证/困难绕过必答/发现必答）+ `work_summary` 事件 + `/summary` 交互——逐单元反思格式底座已在；② 决策轨迹真——`airfc:revised/rejected`、`checkpoint:*`、`work_summary` 都入 Agent 会话事件（单轨）；③ 无聚合——无 `/retro` 把任务级事件聚合为复盘工件；无「有效/无效流程/改进项」分类（缺口）；④ 度量半——修复轮数数据源已接线（`RecordFixAttempt` 由 `RunFixLoopAsync` 消费，`AIDoDOrchestrator.FixRounds` 可查），但无聚合面（门首过率、升版原因分布不可得）；⑤ 无反馈环——复盘改进项无「写回 Wiki/conventions/Skills 或登记新 AIRfc」通道（PDCA 缺 Act）（缺口）；⑥ 先例：harness-self-review.md 已手工演示格式有效，但非产品功能。
- **组件协作**：每单元 → `AIWorkSummary` 五字段 → `work_summary` 事件（真）；`/revise` → `airfc:revised` / `checkpoint:*` → 会话事件单轨（真）；`/retro` → 聚合 → 复盘工件（缺口）；`RecordFixAttempt` → 修复轮数度量（数据源已接线，聚合面待 `/retro`）；改进项 → `AIWiki`/conventions 反馈环（缺口）。
- **能力面**：逐单元小结 + 决策轨迹真底座。结论：应进体系，且底座已备、最小补件轻。
- **诚实缺口**：缺聚合/分类、度量、反馈环。
- **契约对照**：work-summary.md 定义「每工作单元小结 → 偏差判定」主表面（真）；但无任务级复盘工件定义；D7 完成后无复盘产物；`RecordFixAttempt` 已接线（修复轮数数据可查），但 `/retro` 聚合与度量面未落地。
- **能力面（补件）**：① `/retro <rfcId>`：聚合该 AIRfc 全部 `work_summary` / `airfc:*` / `checkpoint:*` / D3 门事件 → 生成结构化复盘（有效流程/无效流程/改进项，对标 Start/Stop/Continue）→ `airfc:retro` 事件入轨迹；② 度量面：门首过率 / 修复轮数（`RecordFixAttempt` 已接线，`AIDoDOrchestrator.FixRounds` 可查）/ 升版次数与原因分布 → 入复盘与 `/status`（2.2 补件）；③ 反馈环：复盘改进项写回 `AIWiki` / `.arcagent/conventions.md`（`ProjectConventionsProvider` 消费）或登记为新 AIRfc（PDCA 闭环）。

### 4.2 B 组场景推导（B1–B12 · 执行/质量/治理域）

> 判定基准与 A 组同口径。**挂靠与能力归属**（B 组推导 §4.2，挂靠 A 组缺口 / 阶段为后续实施排序参考）：

| B 场景 | 挂靠 A 组缺口 | 阶段归属 | 独立新增点 |
|--------|---------------|----------|-----------|
| B1 CI 集成 | A④（门持久化前置） | 阶段 C 之后 | headless 产物 / CI 适配 / flaky·超时策略 |
| B2 回归策略 | A① K5 未清（K1–K3 已清）· A③ L2 迭代 · A④ 绿点 | 阶段 A → C | 测试↔源码影响图 / 回归基线 / 测试改动检测 |
| B3 主分支保护 | A专项③（P3 未落地面）+ SR-1 `AIPlan.Id` | 阶段 D | 分支/PR/合并门模型（B1 前置） |
| B4 发布管理 | A④（绿点单份/Revision 不联动） | 阶段 C 之后 | Release 聚合 / semver / Changelog / 发布门 / 灰度回滚（全新增） |
| B5 事故/Hotfix | A③（升级人路径） | 阶段 C（升级人闭环）之后 | 严重度分级 / 紧急通道 / 事后闭环 / 事故登记（全新增） |
| B6 测试质量 | A③ L2 迭代 · A① K4 已修/K5 未清 · A专项① 验收对照 | 阶段 A → C | flaky 治理 / 同源检测 / 覆盖率门槛 |
| B7 性能/可观测 | A专项② UI 门（同属「可感知结果」面） | 阶段 E | 基准门 / 运行期可观测验收面（「无观测不验收」） |
| B8 安全合规 | A⑤ 门可信度（同源「门不造假」） | 阶段 E | secret/依赖/交付能力审计 + fail-closed 扩展（全新增） |
| B9 多项目管理 | A④（AIRfc 不持久） | 阶段 C（持久化） | 项目注册表 / 共享预算 / 多实例协调 RFC（部分新增） |
| B10 团队扩展 | A⑤ D7 口子（SR-3） | 阶段 E（人门审计） | 身份/角色/审批链（全新增） |
| B11 AI 降级 | A③ L2 迭代 · A专项③ TotalBudget/波内串行 | 阶段 C/D | 能力探测降级梯 / 超时强制 / 幻觉检测（D5 校验）/ 失败隔离 |
| B12 知识沉淀 | A④（AIRfc 不持久）· A②（Acceptance 结构化） | 阶段 B/C | 知识回写 / ADR / 跨任务注入 / 知识库治理（全新增） |

#### B1 CI/流水线集成（❌ 体系空白 · 新门类）

- **用户动作**：把 Harness 的 DoD 门接入外部 CI（GitHub Actions 等）：提交触发 → 跑门 → 失败/超时/flaky 时给出判定；或把 `/dod` 作为 CI 检查项。
- **运转流程**：**无此流程**——DoD 判定器（`AIDoDOrchestrator.RunAutoGatesAsync`）只活在交互式 REPL 会话内（`/dod` 实时逐门打印），门结果仅当次输出、不落盘；无 headless 非交互入口、无机器可消费的产出（门状态 JSON / 退出码契约 / 逐门明细）；无 CI 适配器（GitHub Checks API、required status check、PR 注释）；`QualityCli` 只是 spawn `arc build/test` CLI，其本身可被外部调用，但「DoD 判定」无 CI 接入面。CI 失败/超时/flaky：无概念——D3 只查退出码、无重试策略、无超时预算、无 flaky 检测（与 B6 同根）。
- **组件协作**：`AIDoDOrchestrator`（进程内编排）· `CodingDoDGateEvaluator`（判定信号）· `QualityCli`（spawn arc CLI）· 无 CI 适配器 / 无门报告产物 / 无 CI 环境探测（clang 缺失时 e2e skip，`if !clang_available() { return; }` 已有，但无「门因环境不可用显式降级」语义）。
- **诚实缺口**：**体系空白**——门判定信号本身（`arc build`/`arc test`）可被 CI 独立调用，但「DoD 作为 CI 门」整体缺：无 headless 模式、无门产物、无 CI 适配、无 flaky/超时策略。D7 人门在 CI 语境下无映射（自然解：D0–D6 自动门 = CI 检查，D7 = PR review approval，该映射不存在）。
- **契约对照**：DoD 文档把 D0–D7 描述为「持续可查的完成度体系」（2.2 已证「进程外无状态」）；「CI 集成」在 043 与 references 无任何面。
- **能力面（补件）**：① headless `/dod --json`（非交互、跳过 D7、输出门状态 JSON + 退出码契约）；② 门结果落盘（与 §5 ⑥ 门持久化同源）；③ CI 适配器（GitHub Actions status check / PR comment，映射 D7↔PR review）；④ flaky/超时治理（重试策略、超时预算、flaky 检测进 D3，与 B6 同源）。

#### B2 回归风险与回归测试策略（⚠️ 部分 · 新门类）

- **用户动作**：改 A 处影响 B 处（跨文件引用）后，期望 Harness 判定回归面并增量验证（只跑受影响测试/只重编受影响子图），而非全量重来。
- **运转流程**：`/dod` → D0 跑 `arc build`（全量 build；`--incremental-report` 粒度=**文件级**，非文档宣称的「受影响子图」）→ D3 按 D3 文档契约「分级解锁」选测试（`改 std/** → 相关 crate 集成 + 受影响 e2e`），但**实现无影响图**：测试选择依赖模型自身推理 + 全量 `arc test`；`.arcgr` 项目级 K1–K3 已清（跨文件符号 + New/Call edges + MethodCall 边）→ 可产出「A→B 调用面」，但「测试 ↔ 源码」影响图仍无实现。回归判定无基线（绿点为单份文件级快照，无「回归对照集」）。
- **组件协作**：D0 信号 `QualityCli`（`--incremental-report` 文件级，K5）· D3 信号 `arc test <proj> --logger json`（退出码判定）· D4 `D4DiffCoverage`（diff↔AIPlan 步骤覆盖，非 test↔source 映射）· `.arcgr`（K1–K3 已清，方法边有数据）。
- **能力面**：D3「分级解锁」为文档契约、绿点可回滚为真。
- **诚实缺口**：① 增量验证只有文件级（K5），「只重编受影响子图」为 SR-2 目标态未落地；② 无测试↔源码影响图（引用图有调用数据，但 test 目标→source 依赖映射未建）；③ 无回归基线 / 全量回归成本表；④ 防降级（改测=纠偏）契约无机器 enforcement（D3 不比对测试文件 diff）；⑤ flaky 无处理（与 B6 同根）。
- **契约对照**：DoD D0「借助代码图做增量验证——只重编受影响子图（SR-2 待排期）」、D3「借助依赖图只跑受影响测试（增量测试选择）」——均为契约文本，无实现；K5 自证 `--incremental-report` 只是 rebuilt_files 计数。（按文档实为两个断点：**K5** 增量粒度文件级 + **K7** D0 warning 未判定，推导以文档为准。）
- **能力面（补件）**：① 测试↔源码影响图（K1–K3 已清达完整项目级引用图，再建 test 目标→source 依赖映射）；② 增量测试选择落地（受影响测试集合替代全量）；③ 回归基线（绿点演进为多版本 + 对照集，全量回归 runbook 与成本表）；④ 测试改动检测（D3 前比对测试文件 diff ↔ 实现 diff，同变更集改测 → 触发纠偏协议）。

#### B3 并行开发与主分支保护（⚠️ 部分 · 新门类）

- **用户动作**：多个 Agent / 多个用户同时开发同一仓库，期望「合并门」（build + test + review 全过才合入主分支）、冲突裁决有规可依。
- **运转流程**：当前并行模型 = 单进程内 P3 并行子代理（任务图 + `MaxConcurrentSubAgents` 节流 + 派发取 Scope ToolPath 租约后到拒绝 + 汇总门唯一权威）；**无分支/PR 模型**——工作直接落在工作树，无「合入主分支」概念；无 merge gate 与 CI/评审系统打通（B1 前置）；合并冲突裁决 = 后到拒绝 + 升级人（N4 禁令禁自动选胜者），`AIMergeTransaction` 两阶段提交（staging 全落 → 统一 Move → 冲突整体回滚）**设计态未实现**，以「汇总门先验 + ToolPath 租约写面互斥」替代；`AIPlan.Id`（SR-1）为稳定 Plan 租约键（`"plan:"+Id`）。
- **组件协作**：`AICoordinator`（三 Kind 一表，后到拒绝）· `AIRfcTaskGraph`（DependsOn/Scope）· `AIParallelCoordinator`（波内 async 交错 = 逻辑并行非真并行）· `RunAggregatedGatesAsync`（汇总门）· 无 git 分支 / PR / 评审角色面。
- **能力面**：冲突织物（租约后到拒绝 + 可审计）为真；分支模型 + 分支级绿点 + `AIMergeTransaction` 两阶段提交（见 [conflict-branch](conflict-branch.md)）；`AIPlan.Id`（SR-1）稳定租约键。
- **诚实缺口**：① 主分支保护 / 合并门为空想（无 PR 模型、无 review 角色映射、无 CI 打通）；② 合并门接线（汇总门 CI/headless + 人评审）为设计态；③ 跨进程共享织物仍缺；④ 跨进程多实例并发写同一仓库无防护——冲突织物明确**单进程非目标**（038 §13），团队扩展时须立 RFC 决策（与 B9/B10 联动）。
- **契约对照**：「主分支保护」「合并门」「并行开发」在 043/references 无任何面；P3 只覆盖「同一任务图内的工作项并行」，不覆盖「多 Agent 会话/多用户并发开发同一仓库」；「`AIMergeTransaction` 两阶段提交」「`TotalBudget` 超限强制收束」为设计态（parallel-subagents §8 未落地面①②④）。
- **能力面（补件）**：① 分支/PR 模型（Harness 工作区 ↔ git 分支映射，WorkItem → PR 草案）；② 合并门 = 汇总门（合并后完整 D0–D7）+ 人评审（D7↔PR review approval，B1 打通）+ CI 状态检查；③ `AIPlan.Id`（SR-1）稳定租约键；④ `AIMergeTransaction` 两阶段提交落地；⑤ 合并冲突整体回滚 + 升级人（沿用 N4，不自动裁决）。

#### B4 发布/里程碑管理（❌ 体系空白 · 新门类）

- **用户动作**：多个 AIRfc 完成后聚合成一个 Release；版本号、Changelog、发布门（质量全绿才发）；灰度/回滚发布。
- **运转流程**：**无此流程**——AIRfc 是单需求聚合根（`Revision` 为需求级版本），无 Release 聚合实体、无「多个 AIRfc → 一个 Release」打包层；无版本号（semver 全库无面）；无 Changelog 生成（commit 为 conventional message，决策轨迹可喂 Changelog 但无此面）；无发布门（「D0–D7 全绿才发」只到 AIRfc/AIPlan 级，无 Release 级全绿检查）；无灰度/渠道发布；无发布级回滚（绿点为工作区文件级多版本快照 `index.json` + checkpoint-<seq>，非发布物级）。plan.md 的里程碑是**文档级规划**，非运行期发布管理。
- **组件协作**：`AIRfcRuntime`（单需求生命周期）· `AICheckpointStore`（文件级绿点）· `scripts/git-sync.ps1`（提交推送，非发布）· 无 Release 实体 / 版本号 / Changelog / 发布门。
- **诚实缺口**：**体系空白**——发布/里程碑管理在 Harness 无任何面。AIRfc 宣称「小型项目管理运行时」止步于单需求 + Revision；「跨任务、跨版本流转」（airfc G1）因不持久化（§5 ⑥）连跨会话都不成立，遑论跨需求聚合。
- **契约对照**：airfc.md「AIRfc = 跨任务、跨版本的需求与交付状态唯一事实源」——无 Release 层承载「跨需求」；043 宣称 Harness 交付「可感知结果」，无「交付物 → 版本 → 发布」的链条。
- **能力面（补件）**：① Release 聚合（AIRfc 集合 + 依赖关系 + 状态机）；② 版本号（semver + 与 AIRfc Revision / 绿点映射）；③ Changelog（决策轨迹/`work_summary` → 用户可读变更日志自动生成）；④ 发布门（Release 全绿才发：各 AIRfc D0–D7 + B7 性能/可观测门若进 DoD）；⑤ 灰度/回滚发布（渠道/标签 + 回滚到上一 Release 绿点）。

#### B5 生产事故与紧急修复（Hotfix）（⚠️ 部分 · 新门类）

- **用户动作**：生产环境出 P0/P1 紧急问题，需最小改动快速止血，绕行完整 DoD；事后补测补文档。
- **运转流程**：基础机制真——快速 `/revise` 升版、绿点回滚、L2 超限升级人、HITL 一等动作；但**无紧急通道**：无 P0/P1 严重度分级、无「受限放行 + 债务登记 + 事后闭环」的合法绕行路径。「Completed ⇔ D0–D7 全勾」与宣称纪律（未经验收不得宣称）无紧急豁免分支，紧急修复只能走与普通任务相同的完整流程（或隐性违规）；无事故登记 / Postmortem / RCA 面。
- **组件协作**：`AIRfcRuntime.Revise`（升版）· `AIHarnessSession.CheckpointRollbackAsync`（回滚）· `AIDoDOrchestrator`（迭代/升级人）· 无严重度模型 / 无 hotfix 门 / 无事故事件 kind / 无复盘模板。
- **能力面**：快速升版 / 回滚 / 升级人机制为真（3.1/3.4 已证）。
- **诚实缺口**：hotfix 治理全缺：无分级、无合法绕行（绕行 = 与宣称纪律冲突）、无「事后补测补文档强制闭环」、无事故登记与复盘。现有机制把「紧急」当作「普通任务」处理，等价于无紧急路径。
- **契约对照**：043 宣称纪律只有「未经验收不得宣称」，没有「紧急场景如何合法临时宣称 + 事后补偿」的通道；事故/复盘在 references 无对应面。
- **能力面（补件）**：① 严重度分级（P0/P1/P2 → 不同路径：P0 允许最小验证放行 + 显式债务）；② 紧急通道（hotfix 受限放行：仅最小改动面、`FIXME`/债务登记入决策轨迹、D5/D7 改为「事后限期补齐」）；③ 事后闭环门（hotfix 后限期内 D0–D7 补全 + 补测补文档 + 复盘入 Wiki，B12 联动）；④ 事故登记（`incident:*` 决策事件 kind + 复盘模板）。

#### B6 测试质量治理（⚠️ 部分 · 新门类）

- **用户动作**：治理 flaky 测试、防止「测试与实现同源自证」、防止测试被改弱（防降级）；D3 当前只退出码，如何防假绿。
- **运转流程**：`/dod` → D3 跑 `arc test <proj> --logger json`（**真实 flag**，4.3 已证）→ `QualityCli.IsGreen` 只查 `exit=0`——**不解析断言明细**：测试「通过但断言被删/被弱化/用例数骤降」无法察觉；无 flaky 检测（无重试、无隔离舱、无 flaky 登记）；无测试↔实现同源检测（testing-first 靠「测试先行锁定 + 改测=纠偏」的**人流程**，实现无机器比对「同变更集改测 ↔ 改实现」）；无覆盖率门槛；L2 自动迭代已接线（`RecordFixAttempt` 由 `RunFixLoopAsync` 消费，2.3/4.3 已闭环）。含 `Main` 的 app 项目 `arc test` 可测，真实项目测试面不再因 Main 冲突必红。
- **组件协作**：D3 信号 `QualityCli`（`--logger json`）· `AIAcceptanceSpec`（`Assertions` 纯文本，无结构化断言）· `AIDoDOrchestrator`（迭代骨架，零调用）· 防降级契约文本（「已批准验收测零擅自削弱；改测 = AIRfc 纠偏」）· 无 flaky / 同源 / 覆盖检测器。
- **能力面**：D3 门槽位与 `--logger json` 为真。
- **诚实缺口**：防假绿能力止于退出码：① 无断言明细解析（假绿面：测试文件被改弱不可察）；② 无 flaky 治理；③ 无「测试与实现同源」机器检测（自证风险靠流程不靠门）；④ 无覆盖率/用例数门槛（含 Main 冲突已修，真实项目 D3 可跑）。
- **契约对照**：DoD D3「断言独立于实现」「防降级门：测试零改动、无回归」——实现只跑退出码，无测试文件 diff 检测、无断言结构校验；testing-first「自证风险防御」为流程契约，非机器门。
- **能力面（补件）**：① D3 结构化解析（`--logger json` 用例级明细：通过/失败/断言数/耗时，SR-2 同源）；② 测试改动检测（测试文件 diff ↔ 实现 diff 关联，同变更集改测触发纠偏协议）；③ flaky 治理（重试策略 + flaky 检测 + 隔离舱 + flaky 登记）；④ 覆盖率/用例数门槛（可选进 DoD，作为「测试没被改弱」的证据之一）。

#### B7 性能与可观测性门（⚠️ 部分 · 新门类）

- **用户动作**：要求基准测试进 D3、日志/指标/追踪进验收（「无观测不验收」）、性能回归在合入前被拦截。
- **运转流程**：D3 只跑行为测试（`arc test`），无基准门（benchmark 不进 DoD）；无运行期可观测性验收面（交付代码必须有日志/指标/健康检查的断言契约不存在）；「无观测不验收」为空想。**进程级可观测性为真**——决策轨迹（`airfc:*` / `checkpoint:*` / `approval`）+ transcript JSONL + `work_summary` 事件，但这是 **Harness 自身过程可观测**，不是「交付物可观测」。性能基线在编译器面存在（H2 gate raw 0.58 / 0.62 / 0.56），但那是编译性能基线，未接线到 DoD 验收面；`KV cache 复用可观测`（设计态）未落地。
- **组件协作**：`SessionEventLog`（JSONL transcript）· `AIDecisionEventKind`（决策轨迹）· `QualityCli`（无基准/性能信号）· 无 metrics/tracing 采集面 · `AIParallelCoordinator.TotalBudget`（仅公式，成本可观测一半）。
- **能力面**：过程可观测性（决策轨迹、transcript、checkpoint 事件）为真、编译器性能基线存在。
- **诚实缺口**：「可观测性/性能进验收」为空想：无基准门、无日志/指标/追踪验收断言、无「无观测不验收」规则。运行期可观测门 / 基准门（D9）仍为设计态。
- **设计态登记**：性能观测与日志筛选能力已有具体设计落盘——[performance-observability](performance-observability.md)（`AIPerfMonitor`：Stopwatch 墙钟 + `rt_proc_get_stats` 内存/CPU + 超时熔断 + 退出信号分类；性能信号模型 / 异常分类表；**增强信号不新开门 → 阶段 E 演进 D9 门**）与 [signal-log](signal-log.md)（`AISignalLog` 分级 / KeySignal 筛选 / `BuildLlmView(tokenBudget)` 字符代理 / `AIToolOutput` 工具输出门面）；演进 P1–P3（plan `PF-1–PF-3`）。
- **契约对照**：043 宣称「可感知结果」「质量来自验证闭环」——对「交付物必须可观测（日志/指标）」无任何门；基准测试只存在于 plan.md 编译器性能基线，未与 DoD 接线。
- **能力面（补件）**：① 基准门（D3 扩展或新门：关键路径基准双跑对比 + 回归阈值，基准基线版本化；**设计态** [performance-observability](performance-observability.md) D9 门）；② 可观测性验收面（Acceptance 结构化面加「交付物含日志/指标/健康检查」必填断言，与 §5 ① Acceptance 结构化同源）；③ 「无观测不验收」规则（缺可观测断言 → D5 证明不可用）；④ 运行期指标收集（性能痕迹入决策轨迹与日志面）。

#### B8 安全与合规门（⚠️ 部分 · 新门类）

- **用户动作**：Secret 泄漏扫描、依赖漏洞、权限审计（capability/越权面）进 DoD；当前 fail-closed 能力面如何扩展。
- **运转流程**：**运行时安全面为真**——`AICapabilitySet.Contains` fail-closed（空能力 false）、PlanGate + `PlanGatedCapabilities` 权限门、HITL 审批、冲突织物；`Arc.Security` 已进基座依赖（绿点哈希用）。但 **DoD 门清单（D0–D7）无安全/合规门**：无 secret 泄漏扫描（产物/diff 中的密钥模式）、无依赖漏洞审计（lockfile → CVE 基线）、无交付代码的能力审计（新增 capability/对外接口/越权面 → 最小权限核查 + D7 人审）。「当前 fail-closed 能力面如何扩展」→ 扩展方向是把 fail-closed 原则从**运行时**延伸到**交付物检查**（扫描器不可用 → 门 Pending 而非 Passed）。
- **组件协作**：`AICapabilitySet`（fail-closed）· `AIPlanGate`（PlanGatedCapabilities 权限）· `AIHarnessSession` 挂 `host.Coordinator`（租约）· D6 `D6AntiPatternScan`（源码级，可扩展安全标记）· 无 secret / 依赖 / 能力审计器。
- **能力面**：Agent 运行时安全面（fail-closed 能力、权限门、HITL）真实。
- **诚实缺口**：DoD 无安全/合规门为体系缺口。安全面语义是「Agent 运行时不越权」，非「交付物安全合规」。
- **契约对照**：DoD 清单 D0–D7 无安全门；references 无安全 SDLC 对应面；`Arc.Security` 仅绿点哈希（工具依赖，非门）。
- **能力面（补件）**：① Secret 泄漏扫描门（产物与 diff 的密钥模式扫描，进 Coding 领域）；② 依赖漏洞审计（lockfile 快照 → CVE 基线对照，版本化）；③ 交付能力审计（diff 中新增 capability/权限/对外接口 → 最小权限 + D7 人审）；④ fail-closed 扩展（扫描器不可用 → 门 `Pending` 而非 `Passed`，复用 `Pending≠Passed` 语义）。

#### B9 多项目/工作区管理（⚠️ 部分 · 新门类）

- **用户动作**：一个用户同时管理多个仓库 / 多个 Harness 实例；AIRfc 是否跨项目；预算/上下文如何共享。
- **运转流程**：**项目级隔离为真**——绿点落 `<project>/target/scratch/arc-checkpoints/`（按项目：index.json + checkpoint-<seq>.json + 大文件 objects/ 副本，3.4 已闭环）、conventions 按项目 `.arcagent/conventions.md`、`AIWorkspace` 按宿主根；`examples/ReviewAgent` 证明基座可在第二领域复用。但**跨项目管理为空想**：AIRfc 已持久化（单项目 `target/scratch/arcagent-state/airfc.json`）但无项目注册表 → 无「AIRfc 跨项目流转」；无项目注册表（repo → conventions → 绿点 → 会话索引）；预算无跨项目共享面（`TotalBudget` 是单次并行运行的公式求和，`AISession.Budget` 尚 internal AG-9 ⬜）；上下文无跨项目复用（KV cache 复用可观测设计态）。**多实例并发写同一仓库无防护**——冲突织物明确非目标「分布式锁 / 跨进程租约」（conflict-fabric §2），单进程 `AIHost` 模型下多 Harness 实例并发写 = 无协调（B3/B10 联动，须立 RFC 决策）。
- **组件协作**：`AIWorkspace`（宿主根/沙箱/git 状态）· `ProjectConventionsProvider`（项目级）· `AICheckpointStore`（项目级绿点）· `SessionEventLog`（会话级 JSONL）· `AIRfcRuntime`（单项目持久化：`Serialize`/`Restore` → `arcagent-state/airfc.json`）· 冲突织物（单进程非目标）。
- **能力面**：项目级隔离为真（绿点/约定/工作区按项目）。
- **诚实缺口**：跨项目管理空想：无跨项目 AIRfc、无项目注册表、无共享预算、无多实例协调（后者是文档化非目标，判为「团队扩展前的明示空白」，不当作已实现能力）。
- **契约对照**：airfc G1「跨任务、跨版本流转」——跨会话已可重建（`/save`/`/resume`，持久化），但跨项目仍无从谈起（无项目注册表）；冲突织物 §2 自证「分布式锁/跨进程租约」为非目标。
- **能力面（补件）**：① AIRfc 持久化 + 项目归属字段（跨会话/跨项目重建，阶段 C 同源）；② 项目注册表（workspace registry：repo → conventions → 绿点 → 会话索引）；③ 共享预算/上下文面（跨项目上下文缓存、预算聚合、AG-9 Budget 外化）；④ 多实例协调决策（若多 Agent 写同一仓库须跨进程租约——与冲突织物非目标冲突，**须立 RFC** 而非默认实现）。

#### B10 团队规模扩展（⚠️ 部分 · 新门类）

- **用户动作**：从 1 用户 → 团队（角色：需求方 / 审查者 / 维护者）；权限与审批链（谁 Approve D7）。
- **运转流程**：D7 是 REPL 内「一次人验收」，确认者 = 「开发者」（collaboration-checkpoints 表「谁确认」列只有开发者——单角色模型）；`approval` 决策事件写入轨迹（可审计到「发生过一次确认」），但**无身份**（确认事件不绑定用户身份）、无角色分离（需求方/审查者/维护者）、无审批链（一人 Confirm 即过 D7，无四眼原则）、无 RBAC 面。SR-3 假确认口子未收（`CompletePlanAfterDoDAsync(d5Confirmed, d7Confirmed)` 布尔覆盖，D-08）。
- **组件协作**：`DirectionLoop.D7AcceptAsync`（一次人验收）· `CollaborationCheckpoints.DetectAsync`（高风险检测）· `ApplyHumanGates`（布尔覆盖，SR-3 口子）· `approval` 事件（无身份字段）· 无角色/权限模型。
- **能力面**：协作确认点协议 + `approval` 事件轨迹为真。
- **诚实缺口**：团队治理空想：无身份/角色/审批链，D7「谁确认」不可审计到人，SR-3 假确认口子未收。当前模型只能支撑「单人使用」。
- **契约对照**：collaboration-checkpoints「谁确认 | 开发者」自证单角色；043 无团队/权限面；self-review D-08 文档自认布尔假确认口子。
- **能力面（补件）**：① 身份模型（session/user identity 绑定 `approval` 事件）；② 角色与权限（Requester/Reviewer/Maintainer 映射到 D5/D7 确认权与 B5 紧急通道放行权）；③ 审批链（高风险 = 至少两人：实现者 + 审查者 approve，四眼原则进 D7）；④ SR-3 收口（人类门确认改走显式可审计事件 + 确认来源，禁裸布尔）。

#### B11 AI 能力边界与降级（⚠️ 部分 · 新门类）

- **用户动作**：模型能力不足 / 超时 / 幻觉时 Harness 降级（人工接管路径）、失败隔离。
- **运转流程**：L2 升级人路径为**文本契约**真（DoD L2 边界：验证信号不可信 / 语义意图冲突 / 验证环境不可用 / 迭代超限 → 升级人；HITL `ProvideInput` 一等动作）；但：① 无能力边界探测（无模型能力清单 / 探测 → 不支持时降级阶梯）；② 超时/预算无强制（`AISession.Budget` internal AG-9 ⬜；`TotalBudget` 只算公式，超限强制收束设计态）；③ 幻觉检测空想（唯一防线 D5 自审为字符串证明、无机器校验——证明引用假测试/编造证据无法察觉）；④ 失败隔离弱（P3 波内 async 交错 = 逻辑并行，子代理在宿主内无容器级隔离与熔断；N1/B5 明示「独立宿主/真并行另立 RFC」）。
- **组件协作**：`AIDoDOrchestrator`（L2 升级路径文本）· `AIHumanRequest` HITL（038 §5）· `D5SelfReview`（字符串证明槽位，无校验）· `AISession.Budget`（internal，AG-9）· `AIParallelCoordinator.TotalBudget`（公式）· 无能力探测 / 降级梯 / 熔断面。
- **能力面**：「升级人」路径与 HITL 一等动作真（`RecordFixAttempt` 驱动的自动迭代已接线，超限升级由 `RunFixLoopAsync` 机器触发）；超时/预算强制收束梯（停止新派发 → wrap-up 注入 → 强制中断 → Failed(BudgetExceeded) + 小结 + 升级人，见 [subagent-management §9](subagent-management.md)）。
- **诚实缺口**：能力降级 / 幻觉检测 / 失败隔离为空想或设计态。
- **契约对照**：DoD「升级人（升级非卡死）」为文本契约；parallel-subagents「`TotalBudget` 超限强制收束」设计态；D5「每条 acceptance 附可执行证明」为人工槽位。
- **能力面（补件）**：① 能力探测与降级梯（探测模型能力 → 不支持时降级较小模型 / 模板化子任务 / 升级人）；② 超时/预算强制（`TotalBudget` 收束落地 + `AISession.Budget` 外化 AG-9 + 单次调用超时策略）；③ 幻觉检测（D5 证明机器校验：证明引用真实测试 + 真实通过证据，与阶段 E 同源）；④ 失败隔离（独立宿主/容器 + 熔断 + 失败舱不拖垮整体，真并行阶段 D）。

#### B12 知识沉淀与复用（⚠️ 部分 · 新门类）

- **用户动作**：任务产生的知识（ADR、模式、教训）回写 Wiki/文档；「学到的」注入下一任务。
- **运转流程**：**注入方向为真**——`AIWiki` / `ProjectConventionsProvider`（`.arcagent/conventions.md` → Rules 层）在方向环注入上下文（2.1 已证）；决策轨迹（`airfc:*` / `checkpoint:*` / `approval`）与 transcript JSONL 可审计。但**沉淀方向为空想**：无知识提取/回写机制（`work_summary` 五字段「困难/绕过/发现」只输出不沉淀）；无 ADR / 模式 / 教训自动生成；无跨任务学习（AIRfc 持久化 + 无全局知识库，「学到的」无法进下一任务）；Wiki 是**只读下行注入**（Provider 不接收回写）；「发现 → 触发评审」契约真，但「评审结论 → 知识」无闭环。
- **组件协作**：`AIWiki` / `ProjectConventionsProvider`（只读注入）· `AIWorkSummary`（五字段，每单元）· `SessionEventLog`（transcript）· `AIDecisionEventKind`（轨迹）· 无知识库 / 无 ADR 生成 / 无回写通道。
- **能力面**：Wiki/conventions 注入与决策轨迹为真。
- **诚实缺口**：知识沉淀/复用为空想：无提取、无回写、无跨任务注入、无知识库治理（AI 生成内容无「人审才入」门）。
- **契约对照**：043 与 references 无知识沉淀面；`AIWiki` 是上下文注入器非知识库；work-summary「发现 → 触发评审」无「结论沉淀」尾部。
- **能力面（补件）**：① 知识回写通道（决策轨迹/小结 → Wiki/ADR 沉淀，人确认后入，复用 D7 审计面）；② 教训提取（复盘/纠偏结论结构化 → 全局 lessons 库，与 B5 事故复盘联动）；③ 跨任务注入（下一任务 prompt 自动注入相关 ADR / 模式 / 历史教训）；④ 知识库治理（去重、版本、可信度标记——AI 生成内容须人审才入，防污染）。

---

### B3′ git 分支迭代与合并（并入 B3 深度子面）

> 细分 6 卡：卡 1 开分支迭代 / 卡 2 分支内开发 / 卡 3 分支合并 / 卡 4 解决冲突 / 卡 5 合并后回归 / 卡 6 多分支并行——**判定**：卡 1（分支模型）/ 卡 2（分支级绿点隔离）/ 卡 3（`git merge` 两阶段提交 + 回滚）已成立；卡 4（三方裁决）/ 卡 5（合并门 CI/headless + 人评审 D7）/ 卡 6（多分支并行编排）仍为设计态。映射结论：**不新增顶级门类**——并入 **B3** 增强（主分支保护协议）+ **A.6**（分支隔离 / 基线所有权子面）+ **3.2**（`AIMergeTransaction` 同根）+ **新子门类「合并时冲突裁决」**（与 ToolPath 租约正交）。机制层上位设计见 [conflict-branch](conflict-branch.md)（方案 B · 三级冲突统一仲裁 + 分支模型）。

- **用户动作**：同一 git 仓库开分支迭代 → 合并 → 解决冲突（`/branch new <rfcId>` → 分支内 `/rfc → /dod` → 合并回 main）。
- **运转流程**：① **分支模型**——`AIBranch` + `AIBranchLease`（同名唯一拒绝 + 基线所有权）+ `AICheckpointStore` 分支键（`arc-checkpoints/<branch>/`）；`AIRfc.BranchId` 绑定字段与 `arc detect` 输出 `git_branch` 仍缺；② **合并门 / PR 面**——合并门 = `RunAggregatedGatesAsync`（合并后总工作区完整 D0–D7）判定器真，但无 git merge 集成调用方 / CI/headless 接线；D7↔PR review 无映射；③ **`AIMergeTransaction` git 版两阶段提交**——staging = 分支 commit · Move = `git merge` · 整体回滚 = `git merge --abort` + 绿点回滚已成立；合并前强制基线绿点 + 合并成功打合并绿点；`checkpoint:merge` 事件仍缺（归 B3）；④ **合并时冲突裁决**——不同分支同时改同文件合法（不在租约语义内），需「合并时裁决」而非「写时互斥」；`AIMergeConflictDetector`（`git status --porcelain` UU/AA 解析）+ 三方视图（base/ours/theirs）+ 升级人裁决（禁自动选胜者）+ `merge:conflict / merge:resolved` 事件仍为设计态；⑤ **合并后回归**——`git merge --abort` 与 `CheckpointRollbackAsync`（3.4 已闭环）的联动未接线；⑥ **多分支并行**——P3 是任务图并行非 git 分支并行，无合并序列编排；跨进程共享织物立 RFC 决策（不默认实现，见 [conflict-branch §6](conflict-branch.md)）。
- **组件协作**：`RunAggregatedGatesAsync`（汇总门真，合并门判定复用）· `AICheckpointStore`（分支键）· `AICoordinator`（单进程登记表；跨分支写冲突不在租约语义内）· `AIBranch` / `AIBranchLease` / `AIMergeController` / `AIMergeTransaction`（[conflict-branch](conflict-branch.md)）· `AIConflictResolver`。
- **能力面**：分支模型 / 合并绿点 / 合并事务回滚（`git merge` 两阶段提交 + `--abort` 回滚）。
- **诚实缺口**：合并门（汇总门 + CI/headless + 人评审 D7）与「合并时冲突裁决」（三方视图 + 人 CCB，禁自动选胜者）仍为设计态。
- **契约对照**：「主分支保护 / 合并门 / 并行开发」在 043 / references 无任何面（B3 已登记）；P3 只覆盖「同一任务图内的工作项并行」，不覆盖「多 Agent 会话 / 多用户并发开发同一仓库」。
- **能力面（补件）**：① `AIBranch` + `AIBranchLease`（同名唯一 + 基线所有权）+ `AICheckpointStore` 分支键（`arc-checkpoints/<branch>/`）；`AIRfc.BranchId` + `arc detect` 输出 `git_branch` 仍缺；② `AIMergeTransaction` 落地为 git 两阶段提交 + 合并前强制基线绿点 + 合并成功打合并绿点；`checkpoint:merge` 事件仍缺（归 B3）；③ 合并门 = 汇总门（合并后完整 D0–D7）+ CI/headless（B1 前置关联）+ 人评审（D7↔PR review）；④ 冲突裁决：`AIMergeConflictDetector`（UU/AA 解析）+ 三方视图 + 升级人裁决（禁自动选胜者）+ `merge:conflict / merge:resolved` 事件；⑤ （SR-1）`AIPlan.Id` 稳定租约键（[conflict-branch §10](conflict-branch.md)）；⑥ 跨进程共享织物立 RFC 决策（不默认实现，见 [conflict-branch §6](conflict-branch.md)）。全部归入 §6 阶段 D / F（方案 B 演进 B1–B4，见 [conflict-branch §9](conflict-branch.md)）。

---

## §5 统一共同缺口汇总

> 合并**原两路推导 TOP5**、**A 组 TOP5** 与 **B 组 TOP5** 的统一缺口，去重归纳为 8 条 + 原执行验收侧专项 4 条。均为推导结论归纳，未发明新架构。⚠️/❌/设计态场景的缺口全部收敛于此（去日期后的**诚实边界**）。

| # | 共同缺口 | 涉及场景 | 契约 / 依据 |
|---|----------|----------|------------|
| ① | **A① Spec 面纯文本无结构化（源头缺口）**：Intention/Design/Acceptance 均纯文本 → 矛盾判定（A.1）、影响分析（A.2/A.5）、范围度量（A.3）、NFR 阈值（A.7）、环境标注（A.8）全部卡死 | A.1–A.8 多数 / 1.1 / 1.3 / 2.1 | `AIAcceptanceSpec.Assertions`（string）· `AIDesignSpec` 文本字段（api-sketch §2） |
| ② | **A② AIRfc 生命周期状态机部分落地**：`Contested`/`Frozen`/`Closed`/`Cancelled` 已入枚举 + 运行时迁移；仍缺「冻结窗口 CCB 治理」（AICR / 影响分析门 / `/freeze` REPL）与「谁/何时可改」的机器控制；D7 通过后不自动 `CloseRfc`（收口无联动） | A.1 / A.2 / A.9 / 2.4 / 3.1 | airfc §4.2 新态边表（Active/Superseded/Rejected/Contested/Frozen/Closed/Cancelled） |
| ③ | **A③ 跨进程/多用户协作面缺失**：冲突织物为宿主内登记表 → A.6 多用户偏空、A.1 跨会话方向冲突无裁决；`AIPlan.Id` 稳定租约键为跨会话基础，跨进程面仍缺 | A.6 / A.1 / B3 / B9 | `AICoordinator`（进程内）· SR-1 `"plan:"+Id` · P3 N4 无落地 |
| ④ | **B① 外部交付管线集成空白（CI / 合并门 / Release）**：DoD 只活在 REPL 会话内，无 headless 产物、无 CI 适配、无分支/PR/合并门模型、无 Release 聚合/版本号/Changelog/发布门/灰度回滚 | B1 / B3 / B4 | `AIDoDOrchestrator.RunAutoGatesAsync`（进程内）· 无门报告产物 · 无 Release 实体 · 冲突织物单进程非目标 |
| ⑤ | **B② 门可信度防假绿 enforcement 缺失**：D3 只退出码（不解析断言、无 flaky、无测试降级检测）、无 secret/依赖/交付能力审计、D5 字符串证明无机器校验、D7 布尔假确认（SR-3） | B6 / B8 / B11 / B7 / 4.1 / 4.3 | `QualityCli.IsGreen` 仅 `exit=0` · `D5SelfReview` 字符串槽位 · `ApplyHumanGates` 布尔覆盖（D-08）· DoD 清单无安全/基准门 |
| ⑥ | **A④/B③ 状态不持久化（跨会话 / 跨项目 / 跨任务）**：AIPlan / 门状态不落盘 → resume 只重放事件、无法被 CI/发布消费、无法跨项目流转、无回归基线、无跨任务学习；AIRfc 已持久化，但 AIPlan/门状态本身仍不落盘 | 2.4 / 3.4 / 2.2 / B1 / B2 / B4 / B9 / B12 | `SessionEventLog`（JSONL 事件重放）· `AICheckpointStore`（多绿点 `index.json` + checkpoint-`<seq>`）· `AIRfcRuntime.Serialize/Restore`（AIPlan 仅内存） |
| ⑦ | **B④ 角色/身份/治理流程空白（审批链 / 事故 / 发布治理）**：单用户单角色（D7 确认者=「开发者」），无 RBAC、无四眼原则、无 P0/P1 严重度与紧急通道、无事故登记/复盘 | B10 / B5 / B4 | collaboration-checkpoints「谁确认 = 开发者」· 无 `incident:*` 事件 · SR-3 口子（D-08） |
| ⑧ | **B⑤/A⑤ 预算/超时/失败隔离与过程工件面弱**：`TotalBudget` 只算、`AISession.Budget` internal（AG-9）、无单次调用超时、无能力探测/降级梯、无基准门、P3 波内串行无容器级隔离、`KV cache 复用可观测` 设计态；`RecordFixAttempt` 已消费（修复轮数有数据源；`/retro` 聚合未落地）、无 `arc baseline`/债登记 | B11 / B7 / B3 / A.4 / A.10 / 2.3 / 3.2 | parallel-subagents §8 未落地面②③ · plan.md AG-9 ⬜ · `AIDoDOrchestrator.as:35`（已消费）· 编译器性能基线未接线 DoD |
| — | **执行验收侧专项①：验收对照机制缺失**（acceptance 条目 ↔ 可执行断言 ↔ 实际行为 无对照）：Acceptance 结构化（`AIAcceptanceSpec.Items`）+ D5 证明机器校验（`D5ProofVerifier` 引用存在性）+ D3 用例级明细（`D3TestReport` 解析 + 防降级 + TestName 对照）已覆盖；残余：D5 深度校验（真绿）/ D3 断言级 diff（B6） | 4.1 / A.7 | 判别性用例 |
| — | **执行验收侧专项②：UI 验收空白**（D0–D7 无 UI 视觉 / 交互门） | 4.2 / A.7 | 门清单无渲染 / 交互断言面 |
| — | **执行验收侧专项③：P3 并行纠偏广播缺失**（Revision 升版不广播给在飞子代理；同波预取假冲突；`TotalBudget` 只算；撤单无在飞收束）：纠偏广播（/revise → `RfcRevisionChanged` 钩子广播 revision-changed → 在飞检查点 + 重对齐 + 租约重验）、同波预取假冲突消除（租约惰性化）、撤单在飞收束（`CancelPendingAsync`）已覆盖；`TotalBudget` 超限强制收束仍为设计态 | 3.1 / 3.2 / 3.5 / A.9 | 判别性用例 |
| — | **执行验收侧专项④：文档状态自检 + 驾驶体验轻量化（N3/N2）**：文档漂移（A2/A4）使 LLM「导航过时、边开边重画」——B2（宣称=事实核对）/B7（挂账三态）机制为文本契约但持续执行靠人自觉；门多（D0–D9 + HITL + 租约）致心智负担 | 2.3 / 3.4 / 4.1 / B7 | **N3 文档状态自检**：checkpoint 绿点落盘时机器自动跑 D6 反模式扫描 + 挂账三态复核（通知非判红），把 B2/B7 从「人守纪律」变「机器触发」；**N2 驾驶体验轻量化**：PF-2 `BuildLlmView` 折叠 + CodingAgentPrompt 精简 + 门 Detail 聚合收敛信息密度（去过度设计 B5）。登记：实现规划 |

---

## §6 目标能力面（阶段 A–H · 依赖排序参考）

> 把最小补件按依赖排序为八个阶段。**依赖** = 前置条件；**进入条件** = 何时可开工；**交付物** = 该阶段的产出面；**验收** = 该阶段的完成信号；**禁止** = 该阶段的红线。阶段划分与排序为推导归纳，未发明新架构；各阶段是否已实现见 §1 各场景诚实缺口（此表为**目标能力面**，不承载实现进度）。

### 阶段 A：编译器 / arcgr 断点（K1–K8）

- **目标**：解除「真实项目必红」前提，编译器 / arcgr 项目级可用（D1/D3 信号可信的根基）。
- **依赖**：无（编译器 / arcgr 专项，与 SR-2 同源）。
- **进入条件**：单目标 Sprint 排期（基础面冻结纪律下须登记为编译器 / arcgr 专项，不属 std 侧债）。
- **交付物**：顶层函数/`Main` 符号收集 + 入口点（K1）；项目模式 inspect（多文件符号合并 + 跨文件 edges，K2）；`MethodCall` 边收集（K3）；`arc test` 排除 `Main` 测试编译（K4）；D0 结构化诊断（`--message-format json`）+ warning 判定（K5/K7）；D4 越界修复（K6）；`arc build` 传参姿势（K8）。
- **验收**：① 含 `Main` 多文件项目 `arc inspect` 有真实引用数据（K1–K3）；② `arc test` 不再因 Main 冲突必红（K4）；③ D0 结构化诊断 + warning 判定（K5/K7）；④ D4 越界修复（K6）；⑤ `arc build` 参数姿势修复（K8）。达成后「真实项目必红」前提解除。
- **禁止**：以 std 侧债名义处理；D1/D3 信号可信前宣称「真实项目全绿」。

### 阶段 B：方向环收敛（Spec 结构化 / 澄清向导 / Acceptance 先行 / design-review 门）

- **目标**：模糊意图被机器引导收敛为可执行交付；Acceptance 结构化 + 可自动锁测（消解 §5 ① 的源头）。
- **依赖**：阶段 A（验收条目可结构化，先要 D3 信号可信）。
- **进入条件**：A 验收达成后。
- **交付物**：`/rfc` 澄清向导；Acceptance 结构化面（场景 + 断言条目 + 结构化 NFR 条目——指标/阈值/量化单位，合并 A.7 补件②）；Acceptance 先行硬门；design-review 硬门；Acceptance → 验收测试自动生成 / 锁测接线。
- **验收**：① `/rfc` 澄清向导把模糊意图收敛为 Acceptance + Design；② 未定义 Acceptance 禁止进入实现宣称（硬门）；③ design-review 硬门；④ Acceptance 结构化 + 验收测试自动锁测（消解 1.1 / 1.3 + A.7 的 Acceptance 量化面）。
- **禁止**：纯文本 acceptance 继续充当「已锁定」宣称；把「意图 → 人手动补齐」当作自动收敛。

### 阶段 C：执行环闭环（L2 迭代接线 / 绿点体系 / 持久化）

- **目标**：L2 自动迭代真实驱动、绿点体系版本化、AIRfc / AIPlan / 门状态跨会话持久（消解 §5 ⑥）。
- **依赖**：阶段 B（L2 迭代以可执行 Acceptance 为输入）+ 阶段 A（失败信号结构化）。
- **进入条件**：B 验收达成后。
- **交付物**：`RecordFixAttempt` 接线（失败 → 结构化回喂 → ≤3 轮 → 超限自动回滚 + 升级人）——`RunFixLoopAsync` / `AIDoDFixFeedback` / `IAIFixRoundProvider` / `AIHarnessSession.RunFixLoopAsync`（绿点前置 + 超限回滚）+ REPL 接线；多绿点保留 + Revision 绑定 + AIRfc/AIPlan 联动回滚——`AICheckpointStore` 多绿点（index.json + checkpoint-<seq>.json + 大文件 objects/ 副本）+ `AIRfcRuntime.RestoreRevision` + `AIPlan.RestoreStatus` + `CheckpointRollbackAsync(checkpointId)` 重载；AIRfc / AIPlan / 门状态持久化 + resume 重建；门结果落盘；D7 假确认口子收口（SR-3）。
- **验收**：① `RecordFixAttempt` 接线（消解 2.3 / 4.3，B6/B11 同根）；② 多绿点保留 + Revision 绑定 + AIRfc / AIPlan 联动回滚（消解 3.4）；③ AIRfc / AIPlan / 门状态跨会话持久化 + resume 重建（消解 2.4 + §5 ⑥）；④ D7 假确认口子收口（SR-3，消解 §5 ⑤ 的 D7 面）。
- **禁止**：无状态「已完成」宣称；以 transcript 重放冒充状态重建。

### 阶段 D：并行纠正（真并行 / 纠偏广播 / 预算 / 跨进程织物 RFC）

> 本阶段由 [子代理管理](subagent-management.md)（方案 A：`AISubAgentManager` reconcile 治理，演进 A1–A5）与 [冲突分支](conflict-branch.md)（方案 B：三级冲突统一仲裁 + 分支模型，演进 B1–B4，前置 S0 = SR-1 `AIPlan.Id` + 状态扩展 + 持久化）承载，见两子项 §9 演进路径（每步验收 = 场景五面推演闭环）。

- **目标**：真并行、Revision 纠偏广播、预算超限强制收束；跨进程协作面立 RFC 决策（消解 §5 ③ 与执行验收侧专项③、场景 3.5 与 B3′）。
- **依赖**：阶段 C（AIRfc 持久化）+ S0（SR-1 `AIPlan.Id` 稳定租约键，P3 前置项）。
- **进入条件**：C 验收达成后。
- **交付物**：真并行宿主（独立宿主 / 容器，或 L2 级并行宿主另立 RFC）；**方案 A A1–A5**——reconcile 循环化 `RunAllAsync` + 租约惰性化（修 3.2 同波预取假冲突）+ 失败/死亡即时释放租约（A1）、生命周期状态机 + Spawn/Interrupt/Resume/Cancel/CancelPending（A2，含撤单取消被吞修复）、决策广播 `revision-changed` + 定向 `work-item-rescope` / `wrap-up` + 旁路注入（A3）、动态派发 + `TotalBudget` 超限强制收束（A4）、成本核算 / 汇总门增强（A5）；**方案 B B1–B3**——`AIPlan.Id` + 状态扩展 + `/conflict` CCB 裁决（B1：`AISpecConflictDetector` 字段级 diff → Contested + `AIConflictRecord`/`AIConflictResolver`（人 CCB 唯一入口）+ `airfc:resolved`/`conflict:*` 事件）、`AIBranch` + `AIBranchLease` + 分支级绿点隔离 + `AIMergeTransaction` git 两阶段提交（B2）、合并门（汇总门 + CI/headless + 人评审）+ 合并前冲突预检 + 三方裁决（B3）；跨进程共享冲突织物 RFC（A.6/B3/B9，方案 B B4 决策点）。
- **验收**：① 真并行（独立宿主 / 容器，或 L2 级并行宿主另立 RFC，不冒充多线程）；② Revision 升版纠偏广播 → 在飞子代理检查点 + 重对齐 + 租约重验（消解 3.1 + A.9 + 场景 3.5，方案 A A2/A3）；③ 同波预取假冲突消除（消解 3.2，方案 A A1）；④ `TotalBudget` 超限强制收束（消解 3.2，方案 A A4）+ `AIMergeTransaction` 两阶段提交（消解 3.2，方案 B B2）；⑤ 分支模型 + 合并门 + 冲突裁决（场景 B3′ 断点消除，方案 B B2/B3）；⑥ 跨进程协作面立 RFC（A.6/B3/B9，冲突织物单进程非目标解除前不得宣称多实例安全，方案 B B4）。
- **禁止**：单进程 async 交错冒充真并行；跨进程租约未立 RFC 前宣称多实例并发安全；禁合并冲突自动选胜者（L2/L3 一律升级人 CCB）。

### 阶段 E：验收机制（验收对照 / UI 门 / 人门审计 / 基准门 / 安全门）

- **目标**：验收对照真实、UI 可判定、人门可审计、NFR 有门（消解 §5 ⑤ 与执行验收侧专项①②、A.7/B7/B8）。
- **依赖**：阶段 B（Acceptance 结构化）+ 阶段 C（门持久化 / 可信度）。
- **进入条件**：D 验收达成后（或按主线优先级调整）。
- **交付物**：验收对照机制（Acceptance 断言 ↔ 测试 / 产物断言 ↔ 实际运行结果对照表）；UI 视觉 / 交互门（渲染快照 / 黄金文件 / 交互断言 / 可访问性规则）；D5 证明机器校验 + 人门审计；D9 基准门（`arc bench` + 回归阈值，基准基线版本化；**设计态落点**：[performance-observability](performance-observability.md) 演进 D9）；D10 安全门（secret / 依赖漏洞 / 交付能力审计 + fail-closed 扩展：扫描器不可用 → Pending 而非 Passed）；「无观测不验收」规则。
- **验收**：① 验收对照机制（消解 4.1 + A.7 ①）；② UI 视觉 / 交互门（消解 4.2）；③ D5 证明机器校验 + 人门审计（证明引用真实测试 + 真实通过证据；消解 §5 ⑤ 的 D5 面）；④ D9/D10 门进 DoD（NFR 进验收，消解 A.7 + B7/B8）；⑤ 「无观测不验收」规则与基准基线版本化（B7）。
- **禁止**：以字符串证明充当验收；「退出码绿」冒充「断言绿」；扫描器不可用时 Passed（fail-closed 扩展为 Pending）。

### 阶段 F：交付管线集成（headless / CI 适配 / Release）

- **目标**：DoD 走出 REPL 会话，接入外部交付管线（消解 §5 ④）。
- **依赖**：阶段 C（门结果落盘）+ 阶段 E（门可信度）+ SR-1（`AIPlan.Id`）。
- **进入条件**：E 验收达成后。
- **交付物**：headless `/dod --json`（非交互、跳过 D7、门状态 JSON + 退出码契约）；CI 适配器（GitHub status check / PR comment，D7↔PR review 映射）；flaky / 超时治理（重试策略、超时预算、flaky 检测）；Release 聚合（AIRfc 集合 + 状态机）、semver 版本号、Changelog 生成、发布门、灰度 / 回滚发布；分支 / PR / 合并门模型（B3）。
- **验收**：① headless 产物机器可消费（CI 可独立判定 D0–D6）；② CI 适配（D0–D6 = status check，D7 = PR review approval）；③ Release 聚合 + 发布门 + Changelog + 灰度 / 回滚（消解 B4）；④ 合并门 = 汇总门 + 人评审 + CI 状态检查（消解 B3）。
- **禁止**：把 REPL 输出当 CI 门；无 flaky / 超时策略的「假绿」合入；无 Release 级全绿的发布宣称。

### 阶段 G：治理域（身份角色审批链 / 事故 Hotfix / CCB 冻结）

- **目标**：单用户单角色 → 团队治理（消解 §5 ②⑦：生命周期状态机 + 身份 / 审批链 / 事故 / 冻结）。
- **依赖**：阶段 E（人门审计）+ 阶段 C（AIRfc 持久化 / 门可信度）。
- **进入条件**：E 验收达成后。
- **交付物**：身份模型（`approval` 事件绑定 user）；角色与权限（Requester/Reviewer/Maintainer → D5/D7 确认权与紧急通道放行权）；审批链（高风险至少两人，四眼原则进 D7）；SR-3 收口（人类门确认改走显式可审计事件 + 确认来源）；严重度分级（P0/P1/P2）+ Hotfix 紧急通道（受限放行 + 债务登记 + 事后限期补全）；事故登记（`incident:*` 决策事件 kind + 复盘模板）；AIRfc 生命周期状态机（`Frozen` + `/freeze`、`Closed`、`Cancelled` + `/cancel` + `AICR`/CCB 裁决 + `airfc:ccb_approved`）。
- **验收**：① 身份 / 角色 / 审批链（消解 B10，D7「谁确认」可审计到人）；② Hotfix 紧急通道 + 事后补测补文档强制闭环 + 事故登记（消解 B5）；③ CCB 冻结：`AIRfcStatus.Frozen` / `Closed` / `Cancelled` + `AICR` + CCB 裁决（消解 A.2 / A.9 + §5 ②）；④ SR-3 收口（禁裸布尔覆盖）。
- **禁止**：一人 Confirm 冒充四眼；绕行 DoD 不登记债务；紧急修复不补测补文档；`/revise` 越过 Freeze / Closed 态。

### 阶段 H：知识多项目（复盘反馈 / 跨项目 / 知识库）

- **目标**：过程工件沉淀 + 跨项目流转 + 知识复用（消解 §5 ⑧ 与 A.10/B9/B12）。
- **依赖**：阶段 C（持久化）+ 阶段 G（事故 / 复盘治理底座）。
- **进入条件**：G 验收达成后。
- **交付物**：`/retro` 复盘聚合（Start/Stop/Continue 结构化 + `airfc:retro` 事件）；度量面（门首过率 / 修复轮数——先接线 `RecordFixAttempt` / 升版原因分布）；复盘改进项反馈环（写回 `AIWiki` / `.arcagent/conventions.md` 或登记新 AIRfc，PDCA 闭环）；项目注册表（workspace registry）+ AIRfc 项目归属字段（跨项目流转）；知识回写通道（决策轨迹 / 小结 → Wiki / ADR，人确认后入）；教训提取（复盘 / 纠偏结论 → 全局 lessons 库，与 B5 事故复盘联动）；跨任务注入 + 知识库治理（去重 / 版本 / 可信度标记，AI 生成内容人审才入）；回归基线 / 对照集（B2）。
- **验收**：① `/retro` 复盘工件 + 度量面（消解 A.10）；② 改进项反馈环（PDCA 闭环，消解 A.10 ⑤）；③ 项目注册表 + AIRfc 跨项目流转（消解 B9）；④ 知识回写 + 跨任务注入 + 人审门（消解 B12）；⑤ 回归基线 / 对照集（消解 B2 ③）。
- **禁止**：AI 生成知识不经人审入库；复盘结论无 Act 环节；跨项目共享预算 / 上下文无治理。

---

## §7 诚实边界映射（能力面 vs 空想/设计态）

> 每个场景区分**能力成立（真）** 与 **缺口（空想 / 设计态 / 体系空白）**；「待验证」项为按登记有效但未现场复验的部件。下表为机制层契约，标注每场景的诚实边界。

| 场景 | 能力面（真） | 空想 / 设计态（缺口） |
|------|--------------|------------------------|
| 1.1 | `/rfc→/revise→/dod→D7` 命令链 + 澄清向导 / Acceptance 先行门 / AIRfc 锚点注入 | design-review 硬门完整清单（当前轻量提示版） |
| 1.2 | `arc new`（`scaffold.rs::scaffold_project`）+ `arc detect`（`scaffold.rs::detect_project`，human/json）+ 默认 Skills | 专用 [AITool] `quality.arc_detect` / `arc_new` 未接线；`--agent` 骨架不生成完整 AgentHost 组合根 |
| 1.3 | 升版轨迹（Revision + 决策事件） | acceptance 自动锁测（纯文本无测试结构） |
| 2.1 | Wiki / conventions 注入；`.arcgr` 项目级引用面（顶层符号+`Main` 入口 / 跨文件符号+New/Call edges / MethodCall 边） | K5/K7/K8 未清（增量粒度 / warning 判定 / 传参姿势） |
| 2.2 | 门实时输出（`/dod` 逐门判定） | `/status` 聚合 / 门持久化；真实项目必红（K5/K7/K8 未清） |
| 2.3 | 门判定（D0–D6 真实信号）+ L2 自动迭代（`RunFixLoopAsync` 消费 `RecordFixAttempt` / 结构化回喂 / 超限回滚绿点升级人） | D0 结构化诊断（SR-2）/ D3 断言 diff 解析（B6/A.7） |
| 2.4 | transcript / Wiki / 绿点 + **AIRfc / AIPlan 可跨会话重建** | AIPlan / 门状态持久化（门状态重跑重算） |
| 3.1 | 单会话 `/revise` + **纠偏广播**（`RfcRevisionChanged` 钩子 → 广播 / 定向 → 在飞检查点 + 重对齐 + 租约重验） | 多事件混合仲裁（S-6，A4/A5） |
| 3.2 | 冲突织物拦截（ToolPath 租约后到拒绝）+ reconcile 循环 + 租约惰性化 + 失败/死亡即时释放租约 | 波内真并行 / `TotalBudget` 超限收束完整版（A4）/ `AIMergeTransaction`（合并事务语义） |
| 3.3 | 升版闭环 + **动态增项**（`AttachItem` / `SpawnAsync` / `SetParallelism`） | 验收测试自动同步 / 工作项重跑处置（`Priority`/`Reprioritize`/`Invalidate` 无） |
| 3.4 | 多绿点历史（`index.json` + checkpoint-`<seq>` + 大文件 `objects/` 副本）+ 回滚联动 AIRfc/Plan（`RestoreRevision` / `RestoreStatus`） | 门状态无持久化面（/dod 重跑重算）；AIPlan 跨会话持久化属 2.4 |
| 3.5 | 并行容器（`AIParallelCoordinator` reconcile 循环）+ 升版闭环 + 冲突织物；**撤单收束**——`CancelPendingAsync` + 修取消被吞 + `AIRfcWorkItemStatus.Cancelled` + `subagent:interrupt`/`airfc:cancelled`；**决策同步**——广播 / 定向 / 旁路注入 / 租约重验；**拉起**——`SpawnAsync` / `AttachItem` / `SetParallelism` | 多事件混合仲裁（S-6）仍缺；`Revise:90` WorkItems 别名；REPL 零接线 |
| 4.1 | Acceptance 结构化（`AIAcceptanceSpec.Items`）+ D5 证明机器校验（`D5ProofVerifier` 引用存在性）+ D3 用例级明细（`D3TestReport` 解析 + 防降级 + TestName 对照） | D5 深度校验（真绿）/ D3 断言级 diff（B6）/ 防降级基线跨会话持久化 |
| 4.2 | — | UI 视觉 / 交互门（D0–D7 无此面，体系空白） |
| 4.3 | D3 `--logger json`（真实 flag）+ 含 `Main` 项目可测 + L2 迭代（`RecordFixAttempt` 消费，同 2.3） | D3 断言级 diff 解析（B6） |
| A.1 | 并发租约拦截 + 顺序矛盾判定（`AISpecConflictDetector` 字段级 diff → Contested + `AIConflictRecord` + `conflict:detected`）+ 人 CCB 裁决（`AIConflictResolver.ResolveAsync` 必须 `resolvedBy` 禁自动选胜者 → 新 Revision 基线 + `conflict:resolved`/`airfc:resolved`；`RejectAsync` → Rejected） | Owner/Priority 字段（RTM 完整追溯，挂账） |
| A.2 | Revision / Superseded 基线骨架 + `Frozen`/`Closed` 等状态入枚举 | CCB 裁决（AICR）/ 影响分析门 / `/freeze` REPL（无） |
| A.3 | D4 越界检测（`D4DiffCoverage` 真） | 范围度量 / 蔓延阈值门 / NonGoals（无） |
| A.4 | D6 债扫描 + 约定/Wiki 注入 + 绿点快照 | `arc baseline` / characterization / 债登记（无） |
| A.5 | ABI 人确认点 + 绿点回滚 | 依赖升级规划 / 机器影响分析 / SemVer 校验 / 子图回归（无） |
| A.6 | 单宿主冲突织物 + 汇总门唯一权威 | 跨进程织物 / 基线所有权裁决 / 分支隔离（无）；`AIMergeTransaction`（git 两阶段提交） |
| A.7 | `AIDoDGateKind` 扩展机制 + D6 扫描同构 + AG-4 fail-open 已修 | 基准 / 安全 / 可访问性门、NFR 量化面（无）；**性能子面**：[performance-observability](performance-observability.md)（P1 采集 + D9 门设计态）· [signal-log](signal-log.md) |
| A.8 | L2「环境不可用→升级人」规则 + e2e skip 行为 | 环境就绪门 / mock 分级 / 测试数据管理（无） |
| A.9 | `RejectRfc` + 绿点回滚 + 事件轨迹 + **撤单语义/在飞收束**：`AIRfcStatus.Cancelled` / `AIRfcWorkItemStatus.Cancelled` / `CancelPendingAsync` 收束 + 修取消被吞 | `/cancel` REPL 接线 / 代码处置选项（keep-wip/rollback）/ `airfc:reopened`（无） |
| A.10 | `work_summary` 五字段 + 决策轨迹单轨 | `/retro` 聚合 / 度量 / 反馈环（无） |
| B1 | — | headless 产物 / CI 适配 / 门报告产物 / flaky·超时策略（全无） |
| B2 | D3「分级解锁」契约 + 绿点可回滚 | 影响图 / 增量测试选择 / 回归基线 / 防降级 enforcement（无） |
| B3 | 冲突织物后到拒绝（可审计）+ 分支模型 + 分支级绿点 + `AIMergeTransaction` 两阶段提交 + `AIPlan.Id`（SR-1）稳定租约键 | 分支 / PR / 合并门（CI/PR + 人评审）；跨进程共享织物 |
| B3′ | 分支模型 / 合并绿点 / 合并事务回滚（`git merge` 两阶段提交 + `--abort` 回滚）；汇总门判定器真 | 合并门（汇总门 + CI/headless + 人评审）/ 合并时冲突裁决（三方视图 + 人 CCB） |
| B4 | — | Release 聚合 / semver / Changelog / 发布门 / 灰度回滚（全无） |
| B5 | 快速升版 / 回滚 / 升级人机制 | 严重度分级 / 紧急通道 / 事后闭环 / 事故登记（无） |
| B6 | D3 `--logger json`（真实 flag） | 断言明细解析 / flaky / 同源检测 / 覆盖率（无） |
| B7 | 决策轨迹可观测 + 编译器性能基线 | 基准门（D9）/ 运行期可观测验收面 / `KV cache 复用可观测`（设计态） |
| B8 | 运行时 fail-closed 能力面 + 权限门 + HITL | secret / 依赖 / 交付能力审计门（无） |
| B9 | 项目级隔离（绿点 / 约定 / 工作区按项目）+ 领域二复用 | 跨项目 AIRfc / 项目注册表 / 共享预算 / 多实例协调（非目标） |
| B10 | 协作确认点 + `approval` 事件轨迹 | 身份 / 角色 / 审批链、SR-3 收口（无） |
| B11 | L2 升级人路径 + HITL 一等动作 + 超时/预算强制收束梯（`TotalBudget` 收束：wrap-up → 中断 → Failed(BudgetExceeded) + 升级人） | 能力探测 / 幻觉检测 / 失败隔离（无 / 设计态） |
| B12 | Wiki / conventions 注入 + 决策轨迹 | 知识提取回写 / 跨任务学习 / 知识库治理（无） |

### 待验证项（按登记有效，未现场复验）

- **CD-8 / CD-9**（`||` 短路可空收窄、NLL 嵌套循环误报）修复已登记（plan.md CD 表），修复后冲突织物/汇总门相关用例相应转绿；全量套件现场复验为绿时，场景 3.2 / A.6 / B3 的「冲突织物拦截已修绿」绿证成立——未复验前按「待验证」对待。
- **汇总门已修绿，但 TotalBudget / AIMergeTransaction 设计态**：汇总门 green / red 路径在冲突织物修复后已修绿；但 `TotalBudget` **超限强制收束**与 `AIMergeTransaction` **两阶段提交**仍为设计态（[parallel-subagents §3.3 / §3.5 / §4.5 / §8 未落地面](parallel-subagents.md)），不得把「汇总门绿」延展为「并行成本受控 / 合并事务安全」。

---

## 附：判定基准

> 推导为**只读核对**：判定基准 = 已提交代码 + 文档登记 + e2e 存在性；「能力成立」部件有代码 / e2e 佐证，「待验证」部件以登记为准、未现场复验。

| 面 | 真实证据（为真） | 说明 |
|----|------------------|------|
| 方向环 | `arc_ai_direction_loop_e2e` | `/rfc /revise /summary /dod /reject` 全链路 |
| DoD 自动门 | `arc_ai_dod_d1_e2e` · `arc_ai_dod_d2_d4_e2e` · `arc_ai_dod_d6_e2e` | D1 `.arcgr` 判定、D2/D4 源码级扫描、D6 反模式扫描 |
| DoD 修复迭代 | `arc_ai_dod_fix_loop_e2e` · `arc_ai_dod_k2_e2e` | `RecordFixAttempt` ≤3 轮超限回滚 · `.arcgr` 方法边 |
| 验收对照 | `arc_ai_dod_acceptance_e2e` · `arc_ai_conflict_resolve_e2e` | D5 证明校验 + D3 用例级明细 · L2 矛盾检测 + 人 CCB 裁决 |
| 方向/计划持久化 | `arc_ai_rfc_persistence_e2e` · `arc_ai_plan_persistence_e2e` | AIRfc / AIPlan 跨会话重建 |
| 并行织物 | `arc_ai_parallel_subagents_e2e` · `arc_ai_cancel_pending_e2e` · `arc_ai_decision_sync_e2e` | 租约后到拒绝 · 撤单收束 · 纠偏广播 |
| 绿点回滚 | `arc_ai_checkpoint_multi_greenpoint_e2e` · `arc_ai_checkpoint_rollback_e2e` | 多绿点 + 大文件内容寻址恢复 + AIRfc/Plan 联动 |

---

[返回 043(../../043-harness.md) · [场景闭环推演验收协议](scenario-drive-acceptance.md) · [references 索引](index.md)