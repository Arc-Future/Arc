// HtmlEncoder —— HTML 上下文安全编解码（Arc 核心 Text 编解码族）。
// 对齐 Go html/template 上下文感知安全转义：
// 模板绑定（{{ }} / attr={ }）由编译器注入本编码器调用，XSS 由框架兜底；
// a-html={ }（显式原始 HTML）不经此编码。零 ABI、纯 Arc 实现（复用 StringBuilder）。
namespace Arc.Text;

/// <summary>HTML 上下文安全转义（XSS 兜底）。Encode 用于文本内容，EncodeAttribute 用于属性值。</summary>
public static class HtmlEncoder {
    /// <summary>文本上下文转义：`&amp;` `&lt;` `&gt;`。无特殊字符时原样返回（零分配快速路径）。</summary>
    public static string Encode(string value) {
        if (value == null) {
            return "";
        }
        int n = value.Length;
        bool needs = false;
        for (int i = 0; i < n; i++) {
            char c = value[i];
            if (c == '&' || c == '<' || c == '>') {
                needs = true;
                break;
            }
        }
        if (!needs) {
            return value;
        }
        StringBuilder sb = new StringBuilder();
        for (int i = 0; i < n; i++) {
            char c = value[i];
            if (c == '&') {
                sb.Append("&amp;");
            } else if (c == '<') {
                sb.Append("&lt;");
            } else if (c == '>') {
                sb.Append("&gt;");
            } else {
                sb.Append(c);
            }
        }
        return sb.ToString();
    }

    /// <summary>属性上下文转义：`&amp;` `&lt;` `&gt;` `&quot;` `&#39;`（属性值内引号必须实体化）。</summary>
    public static string EncodeAttribute(string value) {
        if (value == null) {
            return "";
        }
        int n = value.Length;
        bool needs = false;
        for (int i = 0; i < n; i++) {
            char c = value[i];
            if (c == '&' || c == '<' || c == '>' || c == '"' || c == '\'') {
                needs = true;
                break;
            }
        }
        if (!needs) {
            return value;
        }
        StringBuilder sb = new StringBuilder();
        for (int i = 0; i < n; i++) {
            char c = value[i];
            if (c == '&') {
                sb.Append("&amp;");
            } else if (c == '<') {
                sb.Append("&lt;");
            } else if (c == '>') {
                sb.Append("&gt;");
            } else if (c == '"') {
                sb.Append("&quot;");
            } else if (c == '\'') {
                sb.Append("&#39;");
            } else {
                sb.Append(c);
            }
        }
        return sb.ToString();
    }
}