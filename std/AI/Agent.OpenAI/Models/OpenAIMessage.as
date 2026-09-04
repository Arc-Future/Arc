// RFC 038: OpenAI API message DTO（public：见 OpenAIResponse 挂账说明）。
namespace Arc.Agent.OpenAI;
using Arc.Collections;
using Arc.Text.Json;

/// <summary>
/// 助手消息（非流式响应中的 message 字段）。
///  JSON: {"role":"assistant","content":"...","reasoning_content":"...","tool_calls":[...]}
/// reasoning_content 为前向兼容解析（OpenAI 官方不发射；兼容网关可能透出），
/// 与正式 <see cref="Content"/> 严格分离——推理输出绝不混入正式内容。
/// </summary>
public class OpenAIMessage : IJsonDeserializable {
    public string Role;
    public string Content;
    public string ReasoningContent;
    public List<OpenAIToolCall> ToolCalls;

    public OpenAIMessage() {
        this.Role = "";
        this.Content = "";
        this.ReasoningContent = "";
        this.ToolCalls = new List<OpenAIToolCall>();
    }

    /// <summary>JSON 反序列化：role/content/reasoning_content/tool_calls。</summary>
    public void ReadJson(JsonReader reader) {
        while (reader.Read()) {
            if (reader.TokenType == JsonTokenType.EndObject) {
                return;
            }
            if (reader.TokenType == JsonTokenType.PropertyName) {
                string prop = reader.GetString();
                reader.Read();
                if (prop == "role") {
                    this.Role = reader.GetString();
                } else if (prop == "content") {
                    this.Content = reader.GetString();
                } else if (prop == "reasoning_content") {
                    this.ReasoningContent = this.ReasoningContent + reader.GetString();
                } else if (prop == "tool_calls") {
                    this.ToolCalls = this.ReadToolCalls(reader);
                } else {
                    reader.Skip();
                }
            }
        }
    }

    private List<OpenAIToolCall> ReadToolCalls(JsonReader reader) {
        List<OpenAIToolCall> list = new List<OpenAIToolCall>();
        reader.Read();
        while (reader.TokenType != JsonTokenType.EndArray) {
            OpenAIToolCall tc = new OpenAIToolCall();
            tc.ReadJson(reader);
            list.Add(tc);
            reader.Read();
        }
        return list;
    }
}
