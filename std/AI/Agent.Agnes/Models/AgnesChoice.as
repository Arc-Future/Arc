// RFC 038: Agnes API choice DTO（internal，仅服务 Provider 反序列化）。
namespace Arc.Agent.Agnes;
using Arc.Collections;
using Arc.Text.Json;

/// <summary>
/// 单个补全选项。
///  非流式: {"index":0,"message":{...},"finish_reason":"stop"}
///  流式: {"index":0,"delta":{...},"finish_reason":null}
/// message / delta 二选一，缺席方为 null。
/// </summary>
internal class AgnesChoice : IJsonDeserializable
{
    public int Index;
    public string FinishReason;
    public AgnesMessage Message;
    public AgnesDelta Delta;

    public AgnesChoice()
    {
        this.Index = 0;
        this.FinishReason = "";
        this.Message = null;
        this.Delta = null;
    }

    /// <summary>JSON 反序列化：index/finish_reason/message/delta。</summary>
    public void ReadJson(JsonReader reader)
    {
        while (reader.Read())
        {
            if (reader.TokenType == JsonTokenType.EndObject)
            {
                return;
            }
            if (reader.TokenType == JsonTokenType.PropertyName)
            {
                string prop = reader.GetString();
                reader.Read();
                if (prop == "index")
                {
                    this.Index = reader.GetInt32();
                }
                else if (prop == "finish_reason")
                {
                    this.FinishReason = reader.GetString();
                }
                else if (prop == "message")
                {
                    AgnesMessage msg = new AgnesMessage();
                    msg.ReadJson(reader);
                    this.Message = msg;
                }
                else if (prop == "delta")
                {
                    AgnesDelta delta = new AgnesDelta();
                    delta.ReadJson(reader);
                    this.Delta = delta;
                }
                else
                {
                    reader.Skip();
                }
            }
        }
    }
}
