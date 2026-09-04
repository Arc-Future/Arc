namespace Arc.Agent;
using Arc.Collections;
/// <summary>
/// 会话消息。除 role/content 外，承载 OpenAI 兼容工具调用协议所需的两类关联面：
/// <list type="bullet">
/// <item><see cref="ToolCalls"/>：assistant 消息上的 tool_calls 回显（含 id/name/arguments），
/// 多轮工具调用的协议前提——tool 结果消息前必须存在匹配的 assistant tool_calls 消息。</item>
/// <item><see cref="ToolCallId"/>：tool 结果消息关联的 tool_call_id，指向被执行的调用。</item>
/// <item><see cref="ReasoningContent"/>：assistant 消息上的思维链内容（DeepSeek reasoning_content）。
/// 思考模式下**进行了工具调用的轮次**必须在后续请求中完整回传，否则上下文断裂——由会话在
/// 追加 assistant 消息时经 <see cref="AIMessage"/> 保存，Provider 序列化时按需回传。</item>
/// </list>
/// </summary>
public class AIMessage {
    public AIRole Role;
    public string Content;
    /// <summary>多模态内容部件（RFC 038 M5；空列表 = 纯文本消息，文本仍由 <see cref="Content"/> 承载）。</summary>
    public List<AIContentPart> ContentParts;
    /// <summary>assistant 消息承载的 tool_calls 回显（OpenAI 兼容：id/type/function.name/function.arguments）。</summary>
    public List<AIToolCall> ToolCalls;
    /// <summary>tool 结果消息关联的 tool_call_id（OpenAI 兼容 tool 消息必需字段）。</summary>
    public string ToolCallId;
    /// <summary>assistant 消息上的思维链内容（DeepSeek reasoning_content；思考模式工具调用轮次须回传）。</summary>
    public string ReasoningContent;
    public AIMessage() {
        this.Role = AIRole.User;
        this.Content = "";
        this.ContentParts = new List<AIContentPart>();
        this.ToolCalls = new List<AIToolCall>();
        this.ToolCallId = "";
        this.ReasoningContent = "";
    }
    public AIMessage(AIRole role, string content) {
        this.Role = role;
        this.Content = content != null ? content : "";
        this.ContentParts = new List<AIContentPart>();
        this.ToolCalls = new List<AIToolCall>();
        this.ToolCallId = "";
        this.ReasoningContent = "";
    }
    public AIMessage(AIRole role, string content, string toolCallId, List<AIToolCall> toolCalls) {
        this.Role = role;
        this.Content = content != null ? content : "";
        this.ContentParts = new List<AIContentPart>();
        this.ToolCallId = toolCallId != null ? toolCallId : "";
        this.ToolCalls = new List<AIToolCall>();
        if (toolCalls != null) {
            int n = toolCalls.Count;
            int i = 0;
            while (i < n) {
                AIToolCall c = toolCalls[i];
                string cid = c != null && c.CallId != null ? c.CallId : "";
                string name = c != null && c.Name != null ? c.Name : "";
                string args = c != null && c.ArgumentsJson != null ? c.ArgumentsJson : "";
                this.ToolCalls.Add(new AIToolCall(cid, name, args));
                i = i + 1;
            }
        }
        this.ReasoningContent = "";
    }

    /// <summary>OpenAI 兼容 content 字段值：无部件 → 转义后的纯文本字符串；
    /// 有部件 → 多模态 content 数组 JSON。Provider 序列化统一走此入口。</summary>
    public string BuildContentJson() {
        if (this.ContentParts == null || this.ContentParts.Count == 0) {
            return "\"" + AIContentPart.JsonEsc(this.Content) + "\"";
        }
        string arr = "[";
        int n = this.ContentParts.Count;
        int i = 0;
        while (i < n) {
            if (i > 0) { arr = arr + ","; }
            arr = arr + this.ContentParts[i].BuildJson();
            i = i + 1;
        }
        arr = arr + "]";
        return arr;
    }
}