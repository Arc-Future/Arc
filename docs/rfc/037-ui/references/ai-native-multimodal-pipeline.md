# AI 原生 · 多模态输入管线（图 → UI 语义 → spec）

> 本子项承载 [037 §10(../../037-ui.md) 的多模态输入能力：把目标图（Figma 导出图、截图、手绘）转化为
> 可校验的 UI spec，经渲染审视迭代逼近高还原。配套：[fidelity-loop](ai-native-fidelity-loop.md)（审视/验收）· [layout-snapshot](ai-native-layout-snapshot.md)（尺子）。

## 1. 管线总览

    # 六步：模型只做语义映射，几何/校验/渲染全归引擎
    ① 版面解析  目标图 → 结构化区块树（矩形 + 类型 + 文本，JSON）
    ② 组件识别  区块 → golden 目录投票映射（组件 + 属性草稿）
    ③ 生成 spec 区块树 + 组件 + token 约束 → ARML/spec 草案
    ④ 校验      arc-ui typeck（组件/属性/绑定/无裸值）→ 结构化诊断
    ⑤ 渲染审视  LivePreviewHost 渲染 → CapturePng → Vision 审视
    ⑥ 修正      审视批评 → 回到 ③（maxRounds 预算）

## 2. 各步契约

### 2.1 版面解析（Layout Parsing）

- 消费：models.Vision.UnderstandAsync（041 §7.5 多模态理解）或通用多模态 LLM。
- 输出：**结构化区块树**（非自由描述）：每个区块 { rect(x,y,w,h), role(文本/按钮/输入/图片/容器), text?, confidence }。
- 约束：模型输出必须符合固定 schema（JSON Schema 强校验，失败重试/拒绝——对齐 034 结构化诊断哲学）；
  区块矩形是模型**估计**，只作语义输入，**几何最终以引擎布局为准**（第四步后由 LayoutSnapshot 校正）。

### 2.2 组件识别（Component Mapping）

- 区块 → golden 目录投票映射：按 rect 宽高比、内容形态、与 golden 的视觉相似度投票选组件。
- 输出：组件名 + 属性草稿（文本→Text/Button、可点→Button、多行文本区→TextBox 等）。
- 未匹配区块 → 诚实缺口（标记 unknown，不硬猜组件）。

### 2.3 生成 spec

- 生成空间受三重约束：组件目录（受限集）+ token 目录（无裸值）+ 属性 schema（类型化）。
- 输出 ARML/spec 草案；禁止自由发明组件名/属性/裸值。

### 2.4 校验（复用 arc-ui typeck）

- 草案 → arc-ui typeck（与静态 ARML 同一校验管线，非第二套）：组件名/属性/绑定路径/无裸值。
- 失败 → 结构化诊断回喂 LLM 自纠（生成→校验→修正），无需人工介入常见错误。

### 2.5 渲染审视（复用保真闭环）

- LivePreviewHost 渲染 → CapturePng → Vision/LLM 审视（结构对齐 + 视觉质量，见 fidelity-loop §3）。
- 审视焦点：区块对齐（引擎布局 vs 目标图区块，矩形级 diff 用 LayoutSnapshot）、间距阶梯、色彩 token、组件 golden 偏差。

## 3. 原则

- **模型只做语义映射，几何/校验/渲染全归引擎**——多模态高保真不靠模型像素级神算，
  靠引擎分担空间推理（业界证据：MLLM 空间推理失败是截图转代码的主要失败模式，见 SCREENCODER / ReLook 研究）。
- 每步输出可校验、可审计：区块树/组件映射/草案/诊断/审视批评全部结构化落盘（决策事件）。
- 单次生成 + 迭代收敛：默认 maxRounds=3（对齐 AISession 预算哲学），不追求一次到位。

## 4. 边界（本子项）

- 不做像素级神算承诺：目标图与还原结果之间以「引擎布局 + 像素指标」客观度量，不依赖模型主观断言。
- 不做反射式识别：组件识别基于 golden 目录（受控），不做任意 DOM/控件探测。
- 管线为框架能力 + 示例工具（用户可自封装 [AITool] 接入会话，对齐 038 §14 纪律）；
