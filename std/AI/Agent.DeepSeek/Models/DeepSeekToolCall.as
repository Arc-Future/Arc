// RFC 038: DeepSeek tool_call 对象 DTO（internal，仅服务 Provider 反序列化）。
namespace Arc.Agent.DeepSeek;
using Arc.Text.Json;

/// <summary>
/// tool_calls[] 元素。
///  非流式: {"id":"...","type":"function","function":{"name":"...","arguments":"..."}}
///  流式起始: {"index":0,"id":"...","type":"function","function":{"name":"...","arguments":""}}
///  流式续段: {"index":0,"function":{"arguments":"..."}}（id/name 缺席，arguments 为累积值）
/// 缺省字段保持默认值（Index=0、Id=""、Function=null）。
/// </summary>
internal class DeepSeekToolCall : IJsonDeserializable {
    public int Index;
    public string Id;
    public DeepSeekFunction Function;

    public DeepSeekToolCall() {
        this.Index = 0;
        this.Id = "";
        this.Function = null;
    }

    /// <summary>JSON 反序列化：index/id/function；未知字段（type 等）跳过。</summary>
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
                } else if (prop == "id") {
                    this.Id = reader.GetString();
                } else if (prop == "function") {
                    DeepSeekFunction fn = new DeepSeekFunction();
                    fn.ReadJson(reader);
                    this.Function = fn;
                } else {
                    reader.Skip();
                }
            }
        }
    }
}
