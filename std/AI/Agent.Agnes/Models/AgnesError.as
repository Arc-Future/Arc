// RFC 038: Agnes API 错误响应 DTO（internal，仅服务 Provider 反序列化）。
namespace Arc.Agent.Agnes;
using Arc.Text.Json;

/// <summary>
/// Agnes 错误响应：{"error":{"message":"...","type":"..."}}。
/// </summary>
internal class AgnesError : IJsonDeserializable
{
    public string Message;
    public string Type;

    public AgnesError()
    {
        this.Message = "";
        this.Type = "";
    }

    /// <summary>JSON 反序列化：error.message / error.type。</summary>
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
                if (prop == "error")
                {
                    this.ReadErrorObject(reader);
                }
                else
                {
                    reader.Skip();
                }
            }
        }
    }

    private void ReadErrorObject(JsonReader reader)
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
                if (prop == "message")
                {
                    this.Message = reader.GetString();
                }
                else if (prop == "type")
                {
                    this.Type = reader.GetString();
                }
                else
                {
                    reader.Skip();
                }
            }
        }
    }
}
