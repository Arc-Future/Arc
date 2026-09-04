// RFC 038: OpenAI API 错误响应 DTO（public：见 OpenAIResponse 挂账说明）。
namespace Arc.Agent.OpenAI;
using Arc.Text.Json;

/// <summary>
/// OpenAI 错误响应：{"error":{"message":"...","type":"...","param":...,"code":...}}。
/// </summary>
public class OpenAIError : IJsonDeserializable {
    public string Message;
    public string Type;

    public OpenAIError() {
        this.Message = "";
        this.Type = "";
    }

    /// <summary>JSON 反序列化：error.message / error.type。</summary>
    public void ReadJson(JsonReader reader) {
        while (reader.Read()) {
            if (reader.TokenType == JsonTokenType.EndObject) {
                return;
            }
            if (reader.TokenType == JsonTokenType.PropertyName) {
                string prop = reader.GetString();
                reader.Read();
                if (prop == "error") {
                    this.ReadErrorObject(reader);
                } else {
                    reader.Skip();
                }
            }
        }
    }

    private void ReadErrorObject(JsonReader reader) {
        while (reader.Read()) {
            if (reader.TokenType == JsonTokenType.EndObject) {
                return;
            }
            if (reader.TokenType == JsonTokenType.PropertyName) {
                string prop = reader.GetString();
                reader.Read();
                if (prop == "message") {
                    this.Message = reader.GetString();
                } else if (prop == "type") {
                    this.Type = reader.GetString();
                } else {
                    reader.Skip();
                }
            }
        }
    }
}
