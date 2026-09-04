# 设计先行与设计评审

> 关联 [043 Coding Agent Harness 工程(../../043-harness.md) §7。本子项定义设计评审的规则与清单，Harness 在 **AIPlan** 门闩前执行设计评审，确保设计质量。设计为 **AIRfc Design** Spec 面；宣称门闩见 [llm-gates](llm-gates.md)。

## §0 宣称门闩

| # | 门闩 |
|---|------|
| 1 | 设计评审必须项未过 → 禁止批准 AIPlan / 禁止开工执行 |
| 2 | 设计变更 = AIRfc Revision+1（经纠偏），禁静默改 Design 面后宣称对齐 |
| 3 | Plan 面 = `AIPlan` 引用；禁平行 `PlanSpec` 冒充评审通过 |

## 设计原则

| 原则 | 定义 | 可执行检查 |
|------|------|-----------|
| 有远见 | 按需求本质推测**风险点**、**扩展点**、**复杂难点**，并在设计中显式应对 | AIPlan / Design 面必须含"风险/扩展/难点"分析及应对，缺项即 Lint 提示 |
| 懂收敛 | 拒绝过度设计、为不存在需求做扩展；将简单问题复杂化是反模式 | D4 diff 越界检测 = 过度设计信号；越界即升级人确认；**B5 门闩**：新增抽象/状态/字段须有 ≥1 真实场景踩到（迷你推演卡），否则不批准 |
| 模块化 | 职责单一、链路清晰；组件职责不重叠，依赖关系单向 | 每 step 职责边界不重叠；链路 = AIPlan 步骤前后依赖可追溯 |
| 零冗余 | 一切代码为业务服务，精准解决业务问题；一行代码不写十行 | 代码图 reachability + 反模式门确保无死代码/未引用 |
| 合理用设计模式 | 策略模式、工厂模式等 23 种设计模式合理应用，降低复杂性 | 仅提示机制（机器判不了"合理"），设计评审留人确认 |

## 设计评审清单

AIPlan 门闩批准前，设计评审清单必须逐项通过：

### 必须项（不过不批准）

```
□ 风险预判：列出 3 个以上针对需求本质推测的风险点及应对措施
□ 扩展点识别：明确当前设计支持哪些可预见的扩展、不扩展哪些
□ 复杂难点：识别 1 个以上实现难点，给出应对方案
□ 模块边界：每种职责只有一条修改路径（单一职责），无重叠
□ 依赖方向：依赖单向（高层→低层），禁止循环依赖
□ 无冗余代码：新增代码全部可 reachable，无未引用符号
□ 防过度设计（B5）：每个新增抽象 / 状态 / 字段必须有 ≥1 真实场景踩到（设计定稿前
  经 [scenario-drive-acceptance](scenario-drive-acceptance.md) 迷你推演卡确认），
  无场景踩到的抽象不批准
```

### 建议项（Lint 提示，待确认）

```
□ 设计模式命名：若使用了已知模式，以模式名字命名（如 XxxFactory、XxxStrategy）
□ 接口粒度：每个接口 ≤5 个方法（超过提示拆分）
□ 类职责：每个类 ≤200 行（超过提示拆分）
□ 文件行数：每个文件 ≤200 行（超过提示拆分）
```

## 远见检查的 LLM 引导

计划工具在创建 plan 时，通过 Lint 引导模型产出远见分析：

```
plan review —— please address before execution:
- Missing risk analysis: the plan must list 3+ risks specific to this task's nature
  (e.g., "ABI breakage risk", "existing test expectations need updating")
- Missing complexity analysis: identify the hardest part of the
  implementation and how you plan to handle it
- Step 1 title is vague ("Fix things") — describe the concrete
  action and the files it touches
```

## 设计评审与 AIPlan 门闩的关系

```
设计评审（Harness 模板）        AIPlan 门闩（框架内置，038）
────────────────────────     ────────────────────────
- 意图/设计/验收 AIRfc Spec 确认  - 计划步骤创建
- 远见/收敛/模块化/零冗余检查       - 批准/拒绝/修订
- 设计模式合理性                    - 写拦截/放行
- 高沟通价值点标记（协作确认点）     - 事件通知
     ↑ 设计评审通过                         ↓ 计划批准
     └─────────────── 批准后执行 ───────────────┘
```

设计评审在 AIPlan 门闩之前，确定"做什么/做成什么样"；AIPlan 门闩在设计与执行之间，确定"按什么步骤做"。