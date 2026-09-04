// RFC 038: Agnes API response DTO（internal，仅服务 Provider 反序列化）。
namespace Arc.Agent.Agnes;
using Arc.Collections;
using Arc.Text.Json;

/// <summary>
/// Agnes 聊天补全响应（非流式与流式单 data: 行共用）。
///  JSON: {"id":"...","object":"chat.completion","created":123,"model":"...","choices":[...],"usage":{...}}
/// usage 仅在 include_usage 流式末块 / 非流式末位出现，缺席为 null。
/// </summary>
internal class AgnesResponse : IJsonDeserializable
{
    public string Id;
    public string Model;
    public List<AgnesChoice> Choices;
    public AgnesUsage Usage;

    public AgnesResponse()
    {
        this.Id = "";
        this.Model = "";
        this.Choices = new List<AgnesChoice>();
        this.Usage = null;
    }

    /// <summary>JSON 反序列化：id/model/choices/usage。</summary>
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
                if (prop == "id")
                {
                    this.Id = reader.GetString();
                }
                else if (prop == "model")
                {
                    this.Model = reader.GetString();
                }
                else if (prop == "choices")
                {
                    this.Choices = this.ReadChoices(reader);
                }
                else if (prop == "usage")
                {
                    AgnesUsage usage = new AgnesUsage();
                    usage.ReadJson(reader);
                    this.Usage = usage;
                }
                else
                {
                    reader.Skip();
                }
            }
        }
    }

    private List<AgnesChoice> ReadChoices(JsonReader reader)
    {
        List<AgnesChoice> list = new List<AgnesChoice>();
        reader.Read();
        while (reader.TokenType != JsonTokenType.EndArray)
        {
            AgnesChoice c = new AgnesChoice();
            c.ReadJson(reader);
            list.Add(c);
            reader.Read();
        }
        return list;
    }
}
