# AI 原生 · 布局快照契约（LayoutSnapshot）

> 本子项承载 [037 §10(../../037-ui.md) 感知基础设施的「尺子」：把引擎算出的布局结果暴露为结构化数据。
> 配套：[live-preview](ai-native-live-preview.md) · [render-capture](ai-native-render-capture.md) · [fidelity-loop](ai-native-fidelity-loop.md)。

## 1. 目标

LLM 与工具需要**真实度量**而非猜测：元素最终矩形、对齐、边距、文本行盒、实际解析的字体字号。
布局由引擎确定性计算（布局算法 + 自适应投影表），快照与引擎布局**同源**，零分歧——
这是「空间推理交给引擎、模型只做语义」原则的量化面（业界证据：MLLM 空间推理失败是截图转代码的主要失败模式）。

## 2. 契约

    namespace Arc.UI.Layout;

    /// <summary>单元素布局结果（矩形为逻辑像素，坐标系：宿主内容区左上为原点，y 向下）。</summary>
    public class LayoutNode {
        public string Name;          // 元素名（x:Name 或生成名）
        public string TypeName;      // 组件类型（如 Button / TextBlock）
        public double X;
        public double Y;
        public double Width;
        public double Height;
        public HorizontalAlignment HAlignment;
        public VerticalAlignment VAlignment;
        public Thickness Margin;
        public Thickness Padding;
        public string FontFamily;    // 实际解析族（含回退结果）
        public double FontSize;      // 实际解析字号
        public int FontWeight;       // 实际解析字重
        public List<TextLineBox> TextLines;  // 文本行盒（仅文本元素）
        public int ZOrder;
        public bool Visible;
        public List<LayoutNode> Children;
    }

    /// <summary>文本行盒：行矩形 + 基线 + 前缀宽度（供命中/对比用）。</summary>
    public class TextLineBox {
        public double X; public double Y; public double Width; public double Height;
        public double Baseline;
        public double PrefixWidth;
    }

    /// <summary>布局快照：宿主根 + 全树（只读；生成于布局完成后）。</summary>
    public class LayoutSnapshot {
        public LayoutNode Root;
        public double ViewportWidth;
        public double ViewportHeight;
    }

获取入口（VisualHost 评审单元）：

    public LayoutSnapshot GetLayoutSnapshot();   // VisualHost 成员（见 visual-host 子项）

## 3. 确定性保证

- 同一 spec + 同一环境快照（idiom/tier/media/density/容器尺寸）→ 同一布局 → 同一 LayoutSnapshot
  （既有投影表确定性规则，见 [037 §5(../../037-ui.md) 自适应布局）。
- 快照从**布局结果**收集（与 RenderTree 同源），非独立二次计算；禁止快照与引擎布局分叉。
- 布局未完成（IsMeasured=false）时取快照 → 显式错误（先 Measure/Arrange 再取），不返回半成品。

## 4. 序列化与消费

| 消费方 | 通道 | 说明 |
|--------|------|------|
| LLM | JSON（与 .arcgr 同风格，L0 渐进披露） | 生成时拿真实度量；审视时与目标图区块矩形级对比 |
| 工具/CLI | JSON / protobuf | arc ui snapshot 命令（复用 inspect 先例） |
| 验收 | 布局 diff | 矩形级对齐容差输入（fidelity-loop §4 渲染期闸门） |

## 5. 边界（本子项）

- 只读快照，不承载变更（改布局走属性/补丁通道，见 [live-preview](ai-native-live-preview.md) ApplyPatch）。
- 不承诺渲染像素（那是 render-capture 的职责）；快照是几何真值，像素是视觉真值，二者正交。
- 文本度量以引擎实际解析为准（含字体回退结果），禁止伪度量（如按字符数估算）。