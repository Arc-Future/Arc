// RFC 038: Agnes API usage DTO（internal，仅服务 Provider 反序列化）。
namespace Arc.Agent.Agnes;
using Arc.Text.Json;

/// <summary>
/// Token 用量统计。缓存字段对齐 Anthropic/Agnes：cache_read_input_tokens（命中）、
/// cache_creation_input_tokens（写入）。
///  JSON: {"prompt_tokens":10,"completion_tokens":20,"total_tokens":30,
///         "cache_read_input_tokens":4,"cache_creation_input_tokens":8}
/// </summary>
internal class AgnesUsage : IJsonDeserializable
{
    public int PromptTokens;
    public int CompletionTokens;
    public int TotalTokens;
    public int CacheReadTokens;
    public int CacheCreationTokens;

    public AgnesUsage()
    {
        this.PromptTokens = 0;
        this.CompletionTokens = 0;
        this.TotalTokens = 0;
        this.CacheReadTokens = 0;
        this.CacheCreationTokens = 0;
    }

    /// <summary>JSON 反序列化：token 计数与缓存命中/写入字段。</summary>
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
                if (prop == "prompt_tokens")
                {
                    this.PromptTokens = reader.GetInt32();
                }
                else if (prop == "completion_tokens")
                {
                    this.CompletionTokens = reader.GetInt32();
                }
                else if (prop == "total_tokens")
                {
                    this.TotalTokens = reader.GetInt32();
                }
                else if (prop == "cache_read_input_tokens")
                {
                    this.CacheReadTokens = reader.GetInt32();
                }
                else if (prop == "cache_creation_input_tokens")
                {
                    this.CacheCreationTokens = reader.GetInt32();
                }
                else
                {
                    reader.Skip();
                }
            }
        }
    }
}
