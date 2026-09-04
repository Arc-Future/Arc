// RFC 038: DeepSeek API delta DTO（internal，仅服务 Provider 反序列化）。
namespace Arc.Agent.DeepSeek;
using Arc.Collections;
using Arc.Text.Json;

/// <summary>
/// 流式响应中的增量内容。
///  JSON: {"role":"assistant","content":"...","reasoning_content":"...","tool_calls":[...]}
/// </summary>
internal class DeepSeekDelta : IJsonDeserializable {
    public string Role;
    public string Content;
    public string ReasoningContent;
    public List<DeepSeekToolCall> ToolCalls;

    public DeepSeekDelta() {
        this.Role = "";
        this.Content = "";
        this.ReasoningContent = "";
        this.ToolCalls = new List<DeepSeekToolCall>();
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
                    this.ReasoningContent = reader.GetString();
                } else if (prop == "tool_calls") {
                    this.ToolCalls = this.ReadToolCalls(reader);
                } else {
                    reader.Skip();
                }
            }
        }
    }

    private List<DeepSeekToolCall> ReadToolCalls(JsonReader reader) {
        List<DeepSeekToolCall> list = new List<DeepSeekToolCall>();
        reader.Read();
        while (reader.TokenType != JsonTokenType.EndArray) {
            DeepSeekToolCall tc = new DeepSeekToolCall();
            tc.ReadJson(reader);
            list.Add(tc);
            reader.Read();
        }
        return list;
    }
}
