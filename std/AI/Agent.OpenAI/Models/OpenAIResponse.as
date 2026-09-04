// RFC 038: OpenAI API response DTO（public：规避 typeck 跨文件 List<internal> 前向引用缺陷，见 docs/plan.md CD 挂账）。
namespace Arc.Agent.OpenAI;
using Arc.Collections;
using Arc.Text.Json;

/// <summary>
/// OpenAI 聊天补全响应（非流式与流式单 data: 行共用）。
///  JSON: {"id":"...","object":"chat.completion","created":123,"model":"...","choices":[...],"usage":{...}}
/// usage 仅在 include_usage 流式末块 / 非流式末位出现，缺席为 null。
/// </summary>
public class OpenAIResponse : IJsonDeserializable {
    public string Id;
    public string Model;
    public List<OpenAIChoice> Choices;
    public OpenAIUsage Usage;

    public OpenAIResponse() {
        this.Id = "";
        this.Model = "";
        this.Choices = new List<OpenAIChoice>();
        this.Usage = null;
    }

    /// <summary>JSON 反序列化：id/model/choices/usage。</summary>
    public void ReadJson(JsonReader reader) {
        while (reader.Read()) {
            if (reader.TokenType == JsonTokenType.EndObject) {
                return;
            }
            if (reader.TokenType == JsonTokenType.PropertyName) {
                string prop = reader.GetString();
                reader.Read();
                if (prop == "id") {
                    this.Id = reader.GetString();
                } else if (prop == "model") {
                    this.Model = reader.GetString();
                } else if (prop == "choices") {
                    this.Choices = this.ReadChoices(reader);
                } else if (prop == "usage") {
                    OpenAIUsage usage = new OpenAIUsage();
                    usage.ReadJson(reader);
                    this.Usage = usage;
                } else {
                    reader.Skip();
                }
            }
        }
    }

    private List<OpenAIChoice> ReadChoices(JsonReader reader) {
        List<OpenAIChoice> list = new List<OpenAIChoice>();
        reader.Read();
        while (reader.TokenType != JsonTokenType.EndArray) {
            OpenAIChoice c = new OpenAIChoice();
            c.ReadJson(reader);
            list.Add(c);
            reader.Read();
        }
        return list;
    }
}
