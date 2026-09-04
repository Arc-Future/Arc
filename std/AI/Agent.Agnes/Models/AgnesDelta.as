// RFC 038: Agnes API delta DTO（internal，仅服务 Provider 反序列化）。
namespace Arc.Agent.Agnes;
using Arc.Collections;
using Arc.Text.Json;

/// <summary>
/// 流式响应中的增量内容。
///  JSON: {"role":"assistant","content":"...","reasoning_content":"...","thinking":{...},"tool_calls":[...]}
/// content 可为字符串或部件数组；reasoning/thinking 与正式 content 严格分离。
/// </summary>
internal class AgnesDelta : IJsonDeserializable
{
    public string Role;
    public string Content;
    public string ReasoningContent;
    public List<AgnesToolCall> ToolCalls;

    public AgnesDelta()
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
    }

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
