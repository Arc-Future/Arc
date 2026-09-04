# AI 原生 · 保真闭环（Fidelity Loop）

> 本子项承载 [037 §10(../../037-ui.md) 的约束空间与迭代机制：token 目录、组件 golden、审视回路、验收协议、人审固化。
> 配套：[live-preview](ai-native-live-preview.md)（渲染面）· [render-capture](ai-native-render-capture.md)（眼睛）· [layout-snapshot](ai-native-layout-snapshot.md)（尺子）· [multimodal-pipeline](ai-native-multimodal-pipeline.md)（输入侧）。

## 1. DesignTokenCatalog 与无裸值校验

### 1.1 语义化 token 目录

- 每个 token 以机器可读 schema 发布：名字 / 角色 / 用途 / 取值范围 / 对比度约束 / 裸值禁令。
- 来源：现有 token 权威（std/UI/Core/Themes/*.arml 色值 + BuiltInTheme 几何/运动 + 自适应 Spacing.*），
  生成语义化目录（L0/L1 渐进披露，注入 LLM 上下文——对齐 arcgr 哲学）。

| 类别 | 示例 | 约束 |
|------|------|------|
| 色值 | Color.Background / Color.Primary / Color.Text.Primary | 语义角色 + 前景/背景对比度下限 |
| 间距 | Spacing.Page（sm/md/lg） | 间距走 token 阶梯，禁任意裸距 |
| 圆角/层级 | CornerRadius / Elevation | 离散档位 |
| 字体 | FontSize / FontWeight | 离散档位 + 实际解析校验 |
| 时长 | motion | 动画时长档位 |

### 1.2 无裸值校验（保真度第一道闸）

- arc-ui typeck 扩展：ARML/spec 中色值/间距/字号/圆角必须引用 token，**禁裸值**；违者编译期/静态校验拒绝。
- 校验失败返回结构化诊断（含 span 与修复建议），供 LLM 自纠（生成→校验→修正闭环）。
- 意义：单一惯用法在生成侧的强制化——v0 靠 prompt 约束，Arc 靠编译期强制。

## 2. 组件 Golden 基准

### 2.1 资产

- ui-goldens/ 目录（与 .arcgr 同级）：每个 Arc 控件 × 主题态（Light/Dark + hover/pressed/disabled）渲染基准图，
  与「token 化属性 schema」捆绑为组件规范描述。
- 生成工具：golden 渲染器（基于 render-capture，headless 可跑）按控件 × 状态矩阵批量产出。

### 2.2 用途

| 场景 | 用法 |
|------|------|
| 生成 | 以组件为原子（不重造按钮），组件间布局/间距才需推理 |
| 审视 | 渲染截图与 golden 比对：「偏差有多大」而非「对不对」 |
| 多模态输入 | 区块 → golden 目录投票映射（组件识别，见 multimodal-pipeline） |

## 3. 审视回路（Look-and-Refine）

    # 生成 → 渲染 → 截图 → 多模态审视 → 批评 → 修正 → 迭代
    spec → LivePreviewHost.LoadSpec → CapturePng → AIImageInput(截图)
         → Vision.UnderstandAsync / LLM 审视（对齐检查清单）→ 结构化批评（JSON）
         → 修正 spec → 迭代（maxRounds 预算）

| 面 | 决策 |
|----|------|
| 检查清单 | 结构（组件/层级）/ 间距（token 阶梯）/ 色彩（token 语义 + 对比度）/ 组件（golden 偏差）/ 布局（快照 vs 目标区块）；按项目声明，入验收协议 |
| 批评输出 | 结构化 JSON（问题 + 元素路径 + 建议补丁），写回 transcript 可审计 |
| 预算 | maxRounds（对齐 AISession 回合预算哲学）；连续无进展回合不消耗预算（对齐 038 AG-H2 先例） |
| 消费 | 038 §14 统一门面直调（Vision）或会话内用户 [AITool] 自封装；框架不预置 UI 审视工具之外的模型工具 |

## 4. 验收协议（三层闸门）

| 层 | 机制 | 断言 |
|----|------|------|
| 编译期 | arc-ui typeck + token 合规 | 组件名/属性/绑定路径合法；**裸值 0** |
| 渲染期 | 布局快照 diff | 矩形级对齐容差内（与目标区块/参考布局比对） |
| 视觉期 | 截图像素指标 | 结构相似度 / 区域 diff 阈值（与目标图/golden 比对） |

- 验收结果入会话决策事件（append-only，审计）。
- **宣称纪律**：未经验收协议不得宣称「高还原 / 高保真」（对齐 RFC 036）。
- 阈值按项目声明（对标 v0 自定义 design system），框架提供默认档位与工具。

## 5. 人审固化（动态 → 静态晋升通道）

- 验证通过 + 人审满意的动态 UI → VisualHost.ExportArml()（spec → ARML 反向 codegen，带来源/版本元数据）
  → 作为静态产品资产进代码库。
- 意义：AI 产物从「运行时会话数据」晋升为「产品面资产」，高保真最终沉淀而非每次重生成；
  与 v0 产品面思路衔接，是 Arc 编译期哲学的延伸（动态 → 静态）。
- ExportArml 产物必须再次通过编译期闸门（typeck + token 合规）方可合入（禁止绕过校验晋升）。

## 6. 边界（本子项）

- 不承诺像素级神算：几何由引擎兜底，模型只做语义映射与批评。
- 不引入第二套审批/门闩：批评与验收均走既有会话/工具回路。
- 截图不无条件进上下文：渐进披露——先布局快照文本，必要时才截图（对齐 037 §10 感知成本原则）。
- token 目录与 golden 的更新走资源链流程（对齐 builtin-theme-resources 编译期聚合），禁双源。
