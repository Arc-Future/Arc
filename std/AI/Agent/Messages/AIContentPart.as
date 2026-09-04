// AIContentPart — 多模态消息内容部件基类（RFC 038 M5 · Messages 层）。
//
// OpenAI 兼容多模态 content 数组：一条消息的 content 可为「纯文本字符串」或
// 「部件数组」（text / image_url 等）。本基类为部件多态根：Type 为 OpenAI
// content 部件类型标记（text / image_url），派生覆写 BuildJson 产出各自
// content 部件对象 JSON，供 Provider 序列化统一拼接（AIMessage.BuildContentJson）。
namespace Arc.Agent;

/// <summary>多模态消息内容部件基类（RFC 038 M5）。</summary>
public abstract class AIContentPart {
    /// <summary>OpenAI content 部件类型标记（text / image_url）。</summary>
    public string Type;

    public AIContentPart(string type) {
        this.Type = type != null ? type : "";
    }

    /// <summary>本部件的 OpenAI content 部件对象 JSON（如 {"type":"text","text":"..."}）。</summary>
    public abstract string BuildJson();

    /// <summary>JSON 字符串转义（Messages 层内部共用；双引号/反斜杠/控制字符）。</summary>
    internal static string JsonEsc(string s) {
        if (s == null) { return ""; }
        string r = "";
        int i = 0;
        while (i < s.Length) {
            string ch = s.Substring(i, 1);
            if (ch == "\"") { r = r + "\\\""; }
            else if (ch == "\\") { r = r + "\\\\"; }
            else if (ch == "\n") { r = r + "\\n"; }
            else if (ch == "\r") { r = r + "\\r"; }
            else if (ch == "\t") { r = r + "\\t"; }
            else { r = r + ch; }
            i = i + 1;
        }
        return r;
    }
}
