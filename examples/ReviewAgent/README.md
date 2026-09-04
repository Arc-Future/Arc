# ReviewAgent — 领域二样例：数据/文档审查 Harness

> RFC 043 **P5**：用**最小的 Arc 代码**建成**第二个领域**（数据/文档审查型 Harness），证明**基座跨领域复用**：
> 「只组装 Agent + 基座 + 领域工具」即可建成新领域，且**零触碰基座**、不反向依赖 Coding 领域。

本项目是 `examples/ArcAgent`（领域一 Coding）的**平行领域样例**。差别只体现在：

- `arc.toml` **不含** `Arc.Agent.Harness.Coding`；
- 领域工具为 `[AITool] review_file / check_consistency`（能力 `review.Run`）；
- 领域判定为 `ReviewDoDGateEvaluator`（`IAIDoDGateEvaluator`），用文档集完备 / 交叉引用一致性做 D0/D3 等价门；
- 其余（`AIHarnessSession` / AIRfc / DoD 门骨架 / 事件单轨 / 冲突织物 / 计划门闩）全部复用基座。

## 一、基座复用性验证结论（P5 · 2026-08-15）

| 验证项 | 结论 |
|--------|------|
| 基座 `rg "Coding" std/AI/Agent.Harness` | **零命中**（含注释）——基座不引用任何 Coding 域符号 |
| 基座 Coding 耦合回修 | `AIHarnessSession.GrantQualityCapability`（焊死 Coding 能力名 `quality.Verify`）已**删除**；能力白名单改由终端工程自行声明（`AgentHost` / `ReviewHost` 各加 `caps.Add(...)`） |
| `cargo run -p arc -- build examples/ReviewAgent` | ✅ 绿 |
| `cargo run -p arc -- build examples/ArcAgent`（基座回修后复跑） | ✅ 绿 |
| e2e | `arc_ai_domain_two_reuse_e2e` 绿（工具自动装配 + 真实执行 + D0/D3 真实判定 + 人类门诚实阻断）；相关 `arc_ai_*` 无回归 |

**挂账**：基座 `AIDoDOrchestrator.AllPassed` 完成策略为 D0–D7 全 Passed；非 Coding 领域不适用门（ReviewAgent 的 D1/D2/D4/D6）保持 Pending → `Completed` 不假绿。领域化完成策略（仅要求适用门全过）为未来基座扩展项，不在 P5 改动基座。

## 二、项目结构

```
examples/ReviewAgent/
├── arc.toml                 # 包清单：依赖 Arc.Agent / Arc.Agent.DeepSeek / Arc.Agent.Harness（不含 Coding）
├── Program.as               # 入口（无 namespace）：组合根装配 + REPL
└── ReviewAgent/             # 根命名空间 ReviewAgent（= package.name）
    ├── Host/
    │   └── ReviewHost.as    # 组合根：Provider + 会话选项（能力白名单 review.Run / fs.Read / fs.Write 计划门闩）
    ├── Prompt/
    │   └── ReviewAgentPrompt.as   # 领域系统指令（文档审查；无 Coding 编程指令）
    ├── Tools/
    │   ├── ReviewChecks.as  # 领域判定逻辑（[AITool] 与 evaluator 同源复用；递归收集 .md / 空文档 / 交叉引用一致性）
    │   └── ReviewTools.as   # 声明式 [AITool]：review_file / check_consistency（能力 review.Run）
    ├── DoD/
    │   └── ReviewDoDGateEvaluator.as  # IAIDoDGateEvaluator：D0 文档集 / D3 一致性；未接线门诚实 Pending
    └── Repl/
        └── ReviewRepl.as    # 薄壳 REPL：方向环命令经 AIHarnessSession 走基座；领域工具直测
```

## 三、工具与门判定

| 工具 / 门 | 能力 / 信号 | 说明 |
|-----------|------------|------|
| `review_file` | `review.Run` | 单文档审查：行数 + TODO/FIXME 标记 |
| `check_consistency` | `review.Run` | 目录级交叉引用一致性：文档集 + 链接数 + 断链清单 |
| D0 等价门 | 文档集完备 | `ReviewDoDGateEvaluator`：无文档 / 空文档 → Failed |
| D3 等价门 | 交叉引用一致 | `ReviewDoDGateEvaluator`：断链 → Failed；全部可解析 → Passed |
| D5 / D7 | 人类门 | `NeedsHuman`（未确认禁放行，`Pending ≠ Passed`） |
| D1 / D2 / D4 / D6 | — | 领域不适用，诚实 `Pending`（不假绿） |

## 四、运行

```bash
# 1. 设置 DeepSeek API 密钥（真实模式）
set ARC_DEEPSEEK_API_KEY=sk-xxx

# 2. 编译
cargo run -p arc -- build examples/ReviewAgent

# 3. 运行（交互式 REPL）
cargo run -p arc -- run examples/ReviewAgent/Program.as
```

REPL 命令：`/rfc <id>` 立项 · `/revise` 升版 · `/summary <label>` 小结 · `/checkpoint <label>` 绿点 ·
`/rollback <reason>` 回滚 · `/dod` 跑 D0–D7 自动门 · `/review <file>` 单文档审查 ·
`/check [folder]` 一致性检查 · 其余输入走模型回合。

> 方向环 / DoD 逻辑全部经 `AIHarnessSession` 走基座（薄组装）；领域差异只在领域工具与领域 evaluator，
> 不在基座——这正是 P5 要验证的复用纪律。
