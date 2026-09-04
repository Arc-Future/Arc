// RFC 038: OpenAI tool_call 的 function 对象 DTO（public：见 OpenAIResponse 挂账说明）。
namespace Arc.Agent.OpenAI;
using Arc.Text.Json;

/// <summary>
/// tool_calls[].function 对象。
///  JSON: {"name":"echo","arguments":"{\"v\":42}"}
/// arguments 为 JSON 字符串值（内嵌 JSON，非对象）。
/// </summary>
public class OpenAIFunction : IJsonDeserializable {
    public string Name;
    public string Arguments;

    public OpenAIFunction() {
        this.Name = "";
        this.Arguments = "";
    }

    /// <summary>JSON 反序列化：name 与 arguments 字符串值。</summary>
    public void ReadJson(JsonReader reader) {
        while (reader.Read()) {
            if (reader.TokenType == JsonTokenType.EndObject) {
                return;
            }
            if (reader.TokenType == JsonTokenType.PropertyName) {
                string prop = reader.GetString();
                reader.Read();
                if (prop == "name") {
                    this.Name = reader.GetString();
                } else if (prop == "arguments") {
                    this.Arguments = reader.GetString();
                } else {
                    reader.Skip();
                }
            }
        }
    }
}
