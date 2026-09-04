// MessageTemplateFormatter —— 结构化日志消息模板格式化器。
namespace Arc.Logging;

using Arc.Text;

/// <summary>
/// 结构化消息模板格式化器。
///
/// 支持语法：
///   - <c>{{</c> / <c>}}</c> —— 转义字面量花括号；
///   - <c>{Name}</c> —— 占位符，占位符按出现顺序绑定 <c>args</c>（第 k 个占位符 → args[k]）；
///   - <c>{Name,alignment}</c> —— 对齐：正数右对齐、负数左对齐，用空格填充。
///
/// 命名占位符的名称仅作语义元数据（供未来结构化采集），绑定一律按出现顺序（对齐 .NET）。
/// 占位符数量超过参数数量时，缺失参数替换为空串。
/// 说明：Arc 当前编译器不支持值类型装箱到 <c>object</c>，故参数统一为 <c>string</c>
/// （调用方用 <c>"" + value</c> 或插值预转字符串）；数值格式子集（如 <c>:X4</c>）后置。
/// </summary>
internal static class MessageTemplateFormatter {
    /// <summary>将消息模板与参数绑定，返回最终格式化文本。</summary>
    public static string Format(string message, ReadOnlySpan<string> args) {
        if (message == null) { return ""; }
        var sb = new StringBuilder();
        int i = 0;
        int n = message.Length;
        int argIndex = 0;
        while (i < n) {
            string ch = message.Substring(i, 1);
            if (ch == "{") {
                if (i + 1 < n && message.Substring(i + 1, 1) == "{") {
                    sb.Append("{");
                    i = i + 2;
                    continue;
                }
                int close = message.IndexOf("}", i + 1);
                if (close < 0) {
                    // 未闭合 '}':按字面输出
                    sb.Append(ch);
                    i = i + 1;
                    continue;
                }
                string token = message.Substring(i + 1, close - (i + 1));
                string alignment = MessageTemplateFormatter._AlignmentOf(token);
                string value = "";
                if (argIndex < args.Length) {
                    value = args[argIndex];
                }
                argIndex = argIndex + 1;
                if (alignment != null && alignment != "") {
                    value = MessageTemplateFormatter._Pad(value, Convert.ToInt32(alignment));
                }
                sb.Append(value);
                i = close + 1;
                continue;
            }
            if (ch == "}") {
                if (i + 1 < n && message.Substring(i + 1, 1) == "}") {
                    sb.Append("}");
                    i = i + 2;
                    continue;
                }
                sb.Append(ch);
                i = i + 1;
                continue;
            }
            sb.Append(ch);
            i = i + 1;
        }
        return sb.ToString();
    }

    /// <summary>从 <c>Name,alignment</c> 令牌提取对齐部分（首个逗号之后）。</summary>
    private static string _AlignmentOf(string token) {
        int comma = token.IndexOf(",");
        if (comma < 0) { return ""; }
        return token.Substring(comma + 1, token.Length - (comma + 1));
    }

    /// <summary>按 alignment 填充：正数右对齐（左补空格）、负数左对齐（右补空格）。</summary>
    private static string _Pad(string value, int width) {
        int len = value.Length;
        if (width >= 0) {
            if (len >= width) { return value; }
            var sb = new StringBuilder();
            for (int k = 0; k < (width - len); k++) { sb.Append(" "); }
            sb.Append(value);
            return sb.ToString();
        }
        int w = -width;
        if (len >= w) { return value; }
        var sb2 = new StringBuilder();
        sb2.Append(value);
        for (int k = 0; k < (w - len); k++) { sb2.Append(" "); }
        return sb2.ToString();
    }
}
