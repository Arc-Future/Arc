// RFC 038: OpenAI API delta DTO（public：见 OpenAIResponse 挂账说明）。
namespace Arc.Agent.OpenAI;
using Arc.Collections;
using Arc.Text.Json;

/// <summary>
/// 流式响应中的增量内容。
///  JSON: {"role":"assistant","content":"...","reasoning_content":"...","tool_calls":[...]}
/// OpenAI 官方 Chat Completions 不发射 reasoning_content（推理不透明，仅经 usage 计数）；
/// 此处保留受保护的 reasoning_content 解析为前向兼容（OpenAI 兼容网关可能透出），
/// 命中时归入 <see cref="ReasoningContent"/>、绝不混入正式 content。
/// </summary>
public class OpenAIDelta : IJsonDeserializable {
    public string Role;
    public string Content;
    public string ReasoningContent;
    public List<OpenAIToolCall> ToolCalls;

    public OpenAIDelta() {
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
