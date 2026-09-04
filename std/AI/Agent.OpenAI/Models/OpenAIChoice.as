// RFC 038: OpenAI API choice DTO（public：见 OpenAIResponse 挂账说明）。
namespace Arc.Agent.OpenAI;
using Arc.Collections;
using Arc.Text.Json;

/// <summary>
/// 单个补全选项。
///  非流式: {"index":0,"message":{...},"finish_reason":"stop"}
///  流式: {"index":0,"delta":{...},"finish_reason":null}
/// message / delta 二选一，缺席方为 null。
/// </summary>
public class OpenAIChoice : IJsonDeserializable {
    public int Index;
    public string FinishReason;
    public OpenAIMessage Message;
    public OpenAIDelta Delta;

    public OpenAIChoice() {
        this.Index = 0;
        this.FinishReason = "";
        this.Message = null;
        this.Delta = null;
    }

    /// <summary>JSON 反序列化：index/finish_reason/message/delta。</summary>
    public void ReadJson(JsonReader reader) {
        while (reader.Read()) {
            if (reader.TokenType == JsonTokenType.EndObject) {
                return;
            }
            if (reader.TokenType == JsonTokenType.PropertyName) {
                string prop = reader.GetString();
                reader.Read();
                if (prop == "index") {
                    this.Index = reader.GetInt32();
                } else if (prop == "finish_reason") {
                    this.FinishReason = reader.GetString();
                } else if (prop == "message") {
                    OpenAIMessage msg = new OpenAIMessage();
                    msg.ReadJson(reader);
                    this.Message = msg;
                } else if (prop == "delta") {
                    OpenAIDelta delta = new OpenAIDelta();
                    delta.ReadJson(reader);
                    this.Delta = delta;
                } else {
                    reader.Skip();
                }
            }
        }
    }
}
