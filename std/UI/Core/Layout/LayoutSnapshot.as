// RFC 037 §10 / ai-native-layout-snapshot：布局快照契约类型。
//
// 与引擎 Measure/Arrange 结果同源；只读几何真值（与 render-capture 像素真值正交）。
// JSON 供 LLM / headless 断言；禁止伪度量（文本行盒经 TextMeasuring / EstimateTextSize）。

namespace Arc.UI.Layout;

using Arc.Collections;
using Arc.Text;
using Arc.UI;

/// <summary>文本行盒：行矩形 + 基线 + 前缀宽度（供命中/对比）。</summary>
public class TextLineBox {
    public double X;
    public double Y;
    public double Width;
    public double Height;
    public double Baseline;
    public double PrefixWidth;

    /// <summary>序列化为 JSON 对象（流式追加到 StringBuilder）。</summary>
    public void AppendJson(StringBuilder sb) {
        sb.Append("{\"X\":");
        sb.Append(this.X.ToString());
        sb.Append(",\"Y\":");
        sb.Append(this.Y.ToString());
        sb.Append(",\"Width\":");
        sb.Append(this.Width.ToString());
        sb.Append(",\"Height\":");
        sb.Append(this.Height.ToString());
        sb.Append(",\"Baseline\":");
        sb.Append(this.Baseline.ToString());
        sb.Append(",\"PrefixWidth\":");
        sb.Append(this.PrefixWidth.ToString());
        sb.Append("}");
    }
}

/// <summary>单元素布局结果（逻辑像素；宿主内容区左上为原点，y 向下）。</summary>
public class LayoutNode {
    public string Name;
    public string TypeName;
    public double X;
    public double Y;
    public double Width;
    public double Height;
    public HorizontalAlignment HAlignment;
    public VerticalAlignment VAlignment;
    public Thickness Margin;
    public Thickness Padding;
    public string FontFamily;
    public double FontSize;
    public int FontWeight;
    public List<TextLineBox> TextLines;
    public int ZOrder;
    public bool Visible;
    public List<LayoutNode> Children;

    /// <summary>序列化为 JSON 对象（流式追加到 StringBuilder）。</summary>
    public void AppendJson(StringBuilder sb) {
        sb.Append("{\"Name\":");
        LayoutNode.AppendJsonString(sb, this.Name);
        sb.Append(",\"TypeName\":");
        LayoutNode.AppendJsonString(sb, this.TypeName);
        sb.Append(",\"X\":");
        sb.Append(this.X.ToString());
        sb.Append(",\"Y\":");
        sb.Append(this.Y.ToString());
        sb.Append(",\"Width\":");
        sb.Append(this.Width.ToString());
        sb.Append(",\"Height\":");
        sb.Append(this.Height.ToString());
        sb.Append(",\"HAlignment\":");
        LayoutNode.AppendJsonString(sb, LayoutNode.HAlignName(this.HAlignment));
        sb.Append(",\"VAlignment\":");
        LayoutNode.AppendJsonString(sb, LayoutNode.VAlignName(this.VAlignment));
        sb.Append(",\"Margin\":");
        LayoutNode.AppendThicknessJson(sb, this.Margin);
        sb.Append(",\"Padding\":");
        LayoutNode.AppendThicknessJson(sb, this.Padding);
        sb.Append(",\"FontFamily\":");
        LayoutNode.AppendJsonString(sb, this.FontFamily);
        sb.Append(",\"FontSize\":");
        sb.Append(this.FontSize.ToString());
        sb.Append(",\"FontWeight\":");
        sb.Append(this.FontWeight.ToString());
        sb.Append(",\"ZOrder\":");
        sb.Append(this.ZOrder.ToString());
        sb.Append(",\"Visible\":");
        if (this.Visible) {
            sb.Append("true");
        } else {
            sb.Append("false");
        }
        sb.Append(",\"TextLines\":[");
        if (this.TextLines != null) {
            for (int i = 0; i < this.TextLines.Count; i++) {
                if (i > 0) {
                    sb.Append(",");
                }
                this.TextLines[i].AppendJson(sb);
            }
        }
        sb.Append("],\"Children\":[");
        if (this.Children != null) {
            for (int c = 0; c < this.Children.Count; c++) {
                if (c > 0) {
                    sb.Append(",");
                }
                this.Children[c].AppendJson(sb);
            }
        }
        sb.Append("]}");
    }

    private static void AppendThicknessJson(StringBuilder sb, Thickness t) {
        sb.Append("{\"Left\":");
        sb.Append(t.Left.ToString());
        sb.Append(",\"Top\":");
        sb.Append(t.Top.ToString());
        sb.Append(",\"Right\":");
        sb.Append(t.Right.ToString());
        sb.Append(",\"Bottom\":");
        sb.Append(t.Bottom.ToString());
        sb.Append("}");
    }

    private static void AppendJsonString(StringBuilder sb, string value) {
        if (value == null) {
            sb.Append("null");
            return;
        }
        sb.Append("\"");
        for (int i = 0; i < value.Length; i++) {
            char ch = value[i];
            if (ch == '"') {
                sb.Append("\\\"");
            } else if (ch == '\\') {
                sb.Append("\\\\");
            } else if (ch == '\n') {
                sb.Append("\\n");
            } else if (ch == '\r') {
                sb.Append("\\r");
            } else if (ch == '\t') {
                sb.Append("\\t");
            } else {
                sb.Append(ch);
            }
        }
        sb.Append("\"");
    }

    private static string HAlignName(HorizontalAlignment a) {
        if (a == HorizontalAlignment.Left) {
            return "Left";
        }
        if (a == HorizontalAlignment.Center) {
            return "Center";
        }
        if (a == HorizontalAlignment.Right) {
            return "Right";
        }
        return "Stretch";
    }

    private static string VAlignName(VerticalAlignment a) {
        if (a == VerticalAlignment.Top) {
            return "Top";
        }
        if (a == VerticalAlignment.Center) {
            return "Center";
        }
        if (a == VerticalAlignment.Bottom) {
            return "Bottom";
        }
        return "Stretch";
    }
}

/// <summary>布局快照：宿主视口 + 根树（只读；生成于布局完成后）。</summary>
public class LayoutSnapshot {
    public LayoutNode Root;
    public double ViewportWidth;
    public double ViewportHeight;

    /// <summary>确定性 JSON（字段顺序固定；供 LLM / headless 断言）。</summary>
    public string ToJson() {
        StringBuilder sb = new StringBuilder(512);
        sb.Append("{\"ViewportWidth\":");
        sb.Append(this.ViewportWidth.ToString());
        sb.Append(",\"ViewportHeight\":");
        sb.Append(this.ViewportHeight.ToString());
        sb.Append(",\"Root\":");
        if (this.Root == null) {
            sb.Append("null");
        } else {
            this.Root.AppendJson(sb);
        }
        sb.Append("}");
        return sb.ToString();
    }
}