# Harness LLM 复盘报告（教训沉淀）

> **性质**：本文件是**复盘报告 / 教训沉淀**，**不是**能力子项设计契约，不参与 references「一子项一文档」的能力目录契约（能力面仍以 [airfc](airfc.md) / [definition-of-done](definition-of-done.md) / [llm-gates](llm-gates.md) 等为准）。本文件记录 Harness / AIRfc / Coding 体系建设过程中暴露的 **LLM 工作方式偏差** 与**改进机制**，供后续会话 / 协作者规避重复踩坑。
> **范围**：Harness / AIRfc / Coding 体系建设全过程沉淀的 LLM 工作方式偏差与改进机制。

## §0 宣称门闩

> 本报告是**教训沉淀，非能力契约**。禁止把下文「已修 / 已闭环」二次夸大为体系「完成 / 终态」；体系完成度以 [043 宣称纪律(../../043-harness.md) 与 [DoD](definition-of-done.md) 为准。

| # | 门闩 |
|---|------|
| 1 | A 踩坑清单 / B 改进机制是**经验沉淀**，不是新 RFC、不另立 RFC 编号 |
| 2 | 「已修」仅指对应踩坑的**具体改进机制**已落地（P0 三项），不构成「体系已完成 / 终态」宣称 |
| 3 | 引用本报告结论时须**同时引用 P0 落点**（[definition-of-done D8](definition-of-done.md) / [llm-gates G-M8](llm-gates.md) / [anchor-correction-protocol 挂账三态](anchor-correction-protocol.md)），禁止只引「教训」不提「落地」 |
| 4 | 禁止以本报告存在为由，宣称「LLM 踩坑已被制度性杜绝」 |

---

## A 踩坑清单（8 类）

> 每条 = 现象 + 证据 + 代价 + 对应机制。证据以 `文件:行` / commit / e2e 名为准，禁止泛泛而谈。

### A1 设计未先推演 → 过度设计

- **现象**：先搭抽象再验证需求，为「不存在的需求」做扩展。
- **证据**：`AIRfcWorkItem` / `BindWorkItem` 一度零消费者（P3 并行子代理未实现时属前置扩展）；`AICheckpointStore`（全文 ≤64KB + SHA256 + 4000 清单上限）复杂度高于绿点最低可证伪需要（[harness-self-review §2-d](harness-self-review.md)）；旧九态 `AIPlanNodeStatus` 约一半抽象多余，事后砍 `Checkpoint` / `Blocked` / `Skipped` 等（[plan-tree §1 裁剪说明](plan-tree.md)）。
- **代价**：删除 / 收敛成本 + 审阅噪音。
- **对应机制**：B5 设计防过度。

### A2 文档代码漂移

- **现象**：文档把「目标态」写成「现状」，实现滞后未在文档标注两态。
- **证据**：D0 信号源文档写 `--message-format json` 结构化诊断 + 增量验证，实现却为退出码 + stderr 文本 + 全量 build（`QualityCli.IsGreen` 仅查 `exit=0`）（[harness-self-review §1-1](harness-self-review.md)）；`AIPlan` 无稳定 Id 时 Plan 租约键用 Goal 合成、两处合成约定不一致（D-02）。
- **代价**：宣称核对失真、后续会话按错误文档执行。
- **对应机制**：B2 宣称=事实核对。

### A3 真实接入未验证

- **现象**：能力依赖真实外部系统（LLM Provider / HTTPS / 外部 CLI / 真实项目 build+test），却只靠 e2e fixture 断言，未跑真实连通。
- **证据**：Provider 请求体 / SSE 解析 e2e 全绿，但真实 key / endpoint 连通冒烟缺位；「真实接入」曾以 fixture 模拟自证。
- **代价**：上线即发现真实协议 / 鉴权 / 流式差异，返工。
- **对应机制**：B3 真实接入冒烟门（D8）。

### A4 声明 ≠ 行为

- **现象**：文档 / 代码注释声明了能力，但行为未兑现，或声明面（`Pending`）被当成已通过。
- **证据**：D0「0 warning」无实现（`QualityCli.IsGreen` 仅查 exit）；D2 未接线项曾有空扫标 `Passed` 风险；`CompletePlanAfterDoDAsync(d5/d7: bool)` 允许程序化假确认人类门（[harness-self-review D-08](harness-self-review.md)）。
- **代价**：假绿、越权宣称。
- **对应机制**：B4 声明=行为扫描（M8 `Pending≠Passed` 已由 `AllPassed` 强制）。

### A5 空想 vs 实现

- **现象**：空想部件（零调用接线 / 未实现门）被当作能力存在。
- **证据**：`AttachRfcToInstructions` 快照式零调用接线；`arc new` / `DetectProject` 曾为空想（后落地）；`RecordFixAttempt` 零调用、`FixBudgetExceeded` 无消费方（L2 迭代无机器驱动）。
- **代价**：场景推演断点（1.1 断 E、1.2 断 B、2.3 断 D），交付判据失真。
- **对应机制**：B1 真实场景推演门（交付判据 = 场景五面推演闭环，非测试全绿）。

### A6 并行失序

- **现象**：并行开发（多会话 / 多切片 / 多 std 改名）未设收敛点，导致构建断裂 / 状态登记滞后 / 命名轴双轨。
- **证据**：A1 落地时工作区存在并行 A2 代码（`AISubAgentState` / `CancelPendingAsync`）且其 e2e 当时 build 红；并行 std/Arc Phase 3 改名未闭环（`_digitValue` 定义 / 调用不一致）致 D0 红（[harness-self-review D-17](harness-self-review.md)）；P\* / H\* 两套里程碑轴混用。
- **代价**：互相踩脚、验收证据与登记错位。
- **对应机制**：B6 并行收敛点。

### A7 验证偏置

- **现象**：验证只覆盖 happy path / 自证 fixture，e2e 绿 ≠ 真实可达。
- **证据**：多个 e2e 在无 clang / 无 arc 二进制环境 `skip`（`if !clang_available() { return; }`），CI 实际执行情况未复验；e2e fixture 断言请求体，但真实 endpoint 冒烟缺位。
- **代价**：验证自证、真实失败被 fixture 掩盖。
- **对应机制**：B3 真实接入冒烟门（e2e fixture 降为回归证据，不替代冒烟）。

### A8 挂账烂尾

- **现象**：挂账项只登记不闭环，无核实点，状态过时 / 漂移。
- **证据**：H-1「待按 AIRfc 收敛」过时、H-4 ⬜ 但代码 + e2e 已提交（[harness-self-review D-10](harness-self-review.md)）；M4 / M6 / M7 完成但证据块缺失（D-07）；`AIPlan.MarkStepDone` 规避注释疑似过期未复核（D-09）。
- **代价**：状态登记失真、纠偏依赖过期信息。
- **对应机制**：B7 挂账追踪闭环。

---

## B 改进机制（B1–B8）

> 每条 = 机制 + 落点 + 状态。**P0 三项（B2 / B3 / B7）已具备能力面**，其余为已存在需强化或设计态。

### B1 真实场景推演门（← A5）

- 交付判据 = 场景五面（A 输入 / B 真实代码路径 / C LLM 视角 / D 工具调用 / E 上下文）推演闭环，非测试全绿。
- 落点：[scenario-drive-acceptance](scenario-drive-acceptance.md)。
- 状态：**能力面已具备**：设计定稿前强制迷你推演卡前置（[§2.1](scenario-drive-acceptance.md)——把五面推演前置到设计前，新增抽象/状态/字段须有 ≥1 真实场景踩到，否则不批准）。

### B2 宣称=事实核对（← A2）

- 合入前逐条「已实现 / 已支持 / 已修 / 全绿」跑一次**可执行核对命令**（`rg` / `arc` / `git`）并留证据，未过禁宣称。
- 落点：**[llm-gates G-M8](llm-gates.md)**（升格）。
- 状态：**能力面已具备**。

### B3 真实接入冒烟门（← A3 / A7）

- 凡涉及真实外部系统（LLM Provider / HTTPS / 外部 CLI / 真实项目 build+test）的能力，`Completed` 前置 = **一次真实连通冒烟**（真实 key / endpoint / 真实项目跑一遍）留 trace；e2e fixture 降为回归证据，不替代冒烟。
- 落点：**[definition-of-done D8](definition-of-done.md)**（新增）。
- 状态：**能力面已具备**。

### B4 声明=行为扫描（← A4）

- 声明面与行为逐条对照，`Pending` 不得当 `Passed`。
- 落点：[definition-of-done D6](definition-of-done.md) + `D6AntiPatternScan`。
- 状态：**能力面已具备**：`D6AntiPatternScan` 追加「疑似死代码（public 符号零引用）+ 宣称待证（宣称符号反向 grep 无实现）」源码级静态扫描（咨询信号，不判红）；可执行用例 `arc_ai_dod_d6_e2e`。

### B5 设计防过度（← A1）

- 动手前先推演真实场景再定抽象；无调用方的抽象不落。
- 落点：[design-review](design-review.md)（远见 / 收敛 / 模块化 / 零冗余）。
- 状态：**能力面已具备**：design-review 必须项新增「防过度设计」门闩——每个新增抽象/状态/字段必须有 ≥1 真实场景踩到（迷你推演卡确认），否则不批准。

### B6 并行收敛点（← A6）

- 并行改动设显式收敛点（构建基线 + 状态登记刷新 + 命名轴单一化），未收敛不宣称完成。
- 落点：[collaboration-checkpoints](collaboration-checkpoints.md) 机器收敛点。
- 状态：**能力面已具备**：并行合并/提交前强制统一 build/test + 统一落盘 + 命名轴单一化，状态可机器断言。

### B7 挂账追踪闭环（← A8）

- 挂账项强制「登记 → 核实（证据链）→ 更新（已修 / 仍挂 / 已过时）」三态 + 下次核实点；「已修」必须带可复验证据（e2e 名 / `文件:行` / 探针）。
- 落点：**[anchor-correction-protocol 挂账三态](anchor-correction-protocol.md)**（新增）。
- 状态：**能力面已具备**。

### B8 提示词纪律

- LLM 提示词显式声明：未经验证不宣称、验证命令留 trace、挂账带核实点、禁止二次夸大「已修」。
- 落点：[CodingAgentPrompt(../../../../examples/ArcAgent/ArcAgent/Context/CodingAgentPrompt.as)。
- 状态：**能力面已具备**：新增「Thinking discipline」三条纪律段——先推演后实现 / 宣称前先验证 / 不加无场景抽象。

---

## C 落地方案（优先级表）

| 优先级 | 项 | 机制 | 落点 | 状态 |
|--------|----|------|------|------|
| P0 | 真实接入冒烟门 | B3 | [definition-of-done D8](definition-of-done.md) | 能力面已具备 |
| P0 | 宣称=事实核对 | B2 | [llm-gates G-M8](llm-gates.md) | 能力面已具备 |
| P0 | 挂账追踪闭环 | B7 | [anchor-correction-protocol 挂账三态](anchor-correction-protocol.md) | 能力面已具备 |
| P1 | 真实场景推演门强化 | B1 | [scenario-drive-acceptance §2.1](scenario-drive-acceptance.md) | 能力面已具备 |
| P1 | 声明=行为扫描脚本化 | B4 | [definition-of-done D6](definition-of-done.md) + `D6AntiPatternScan` | 能力面已具备 |
| P1 | 设计防过度门闩 | B5 | [design-review](design-review.md) | 能力面已具备 |
| P1 | 并行收敛点 | B6 | [collaboration-checkpoints](collaboration-checkpoints.md) | 能力面已具备 |
| P1 | 提示词纪律 | B8 | [CodingAgentPrompt(../../../../examples/ArcAgent/ArcAgent/Context/CodingAgentPrompt.as) | 能力面已具备 |

---

## D 一句话总结

**最该多做 3 件**：

1. 多**真实场景推演**——先推演真实交付路径再设计 / 实现，交付判据 = 五面推演闭环。
2. 多**真实接入冒烟**——真实 key / endpoint / 真实项目跑一遍留 trace，e2e fixture 只作回归证据。
3. 多**宣称核对**——宣称前跑可执行核对命令（`rg` / `arc` / `git`）留证据，未过禁宣称。

**最该少做 3 件**：

1. 少**空想造抽象**——无调用方的抽象、前置扩展、伪实现 stub，一律不落。
2. 少**先声明后验证**——不把目标态写成现状，不拿 `Pending` 当 `Passed`。
3. 少**挂账不闭环**——挂账必带核实点 + 可复验证据，禁止登记即忘。

---

[返回 references 索引](index.md) · [返回 043(../../043-harness.md)
