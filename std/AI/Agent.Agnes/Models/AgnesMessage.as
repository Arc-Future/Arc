// RFC 038: Agnes API message DTO（internal，仅服务 Provider 反序列化）。
namespace Arc.Agent.Agnes;
using Arc.Collections;
using Arc.Text.Json;

/// <summary>
/// 助手消息（非流式响应中的 message 字段）。
///  JSON: {"role":"assistant","content":"...","reasoning_content":"...","thinking":{...},"tool_calls":[...]}
/// content 可为字符串或部件数组（含 type=="reasoning"/"thinking" 部件）。reasoning/thinking
/// 与正式 <see cref="Content"/> 严格分离——推理输出绝不混入正式内容（空 content 但有推理 ≠ 空回复）。
/// </summary>
internal class AgnesMessage : IJsonDeserializable
{
    public string Role;
    public string Content;
    public string ReasoningContent;
    public List<AgnesToolCall> ToolCalls;

    public AgnesMessage()
    {
        this.Role = "";
        this.Content = "";
        this.ReasoningContent = "";
        this.ToolCalls = new List<AgnesToolCall>();
    }

    /// <summary>JSON 反序列化：role/content/reasoning_content/thinking/tool_calls。</summary>
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
                if (prop == "role")
                {
                    this.Role = reader.GetString();
                }
                else if (prop == "content")
                {
                    this.ReadContentValue(reader);
                }
                else if (prop == "reasoning_content")
                {
                    this.ReasoningContent = this.ReasoningContent + reader.GetString();
                }
                else if (prop == "thinking")
                {
                    this.ReasoningContent = this.ReasoningContent + this.ReadThinkingValue(reader);
                }
                else if (prop == "tool_calls")
                {
                    this.ToolCalls = this.ReadToolCalls(reader);
                }
                else
                {
                    reader.Skip();
                }
            }
        }
    }

    /// <summary>读取 content 值：字符串 → 正式文本；部件数组 → 拆分 text（Content）与
    /// reasoning/thinking（ReasoningContent）。推理输出绝不落入 Content。</summary>
    private void ReadContentValue(JsonReader reader)
    {
        if (reader.TokenType == JsonTokenType.String)
        {
            this.Content = this.Content + reader.GetString();
        }
        else if (reader.TokenType == JsonTokenType.StartArray)
        {
            while (reader.Read())
            {
                if (reader.TokenType == JsonTokenType.EndArray)
                {
                    return;
                }
                if (reader.TokenType == JsonTokenType.StartObject)
                {
                    this.ReadContentPart(reader);
                }
            }
        }
        else
        {
            reader.Skip();
        }
    }

    private void ReadContentPart(JsonReader reader)
    {
        string type = "";
        string text = "";
        while (reader.Read())
        {
            if (reader.TokenType == JsonTokenType.EndObject)
            {
                break;
            }
            if (reader.TokenType == JsonTokenType.PropertyName)
            {
                string p = reader.GetString();
                reader.Read();
                if (p == "type")
                {
                    if (reader.TokenType == JsonTokenType.String)
                    {
                        type = reader.GetString();
                    }
                }
                else if (p == "text" || p == "thinking")
                {
                    if (reader.TokenType == JsonTokenType.String)
                    {
                        text = text + reader.GetString();
                    }
                }
                else
                {
                    reader.Skip();
                }
            }
        }
        if (type == "reasoning" || type == "thinking")
        {
            this.ReasoningContent = this.ReasoningContent + text;
        }
        else if (type == "text")
        {
            this.Content = this.Content + text;
        }
        // image_url 等其余部件在输出侧忽略。
    }

    /// <summary>读取 thinking 扩展字段值：字符串或对象（取 thinking/text 字符串字段）。</summary>
    private string ReadThinkingValue(JsonReader reader)
    {
        if (reader.TokenType == JsonTokenType.String)
        {
            return reader.GetString();
        }
        if (reader.TokenType == JsonTokenType.StartObject)
        {
            string r = "";
            while (reader.Read())
            {
                if (reader.TokenType == JsonTokenType.EndObject)
                {
                    break;
                }
                if (reader.TokenType == JsonTokenType.PropertyName)
                {
                    string p = reader.GetString();
                    reader.Read();
                    if (p == "thinking" || p == "text")
                    {
                        if (reader.TokenType == JsonTokenType.String)
                        {
                            r = r + reader.GetString();
                        }
                    }
                    else
                    {
                        reader.Skip();
                    }
                }
            }
            return r;
        }
        reader.Skip();
        return "";
    }

    private List<AgnesToolCall> ReadToolCalls(JsonReader reader)
    {
        List<AgnesToolCall> list = new List<AgnesToolCall>();
        reader.Read();
        while (reader.TokenType != JsonTokenType.EndArray)
        {
            AgnesToolCall tc = new AgnesToolCall();
            tc.ReadJson(reader);
            list.Add(tc);
            reader.Read();
        }
        return list;
    }
}
