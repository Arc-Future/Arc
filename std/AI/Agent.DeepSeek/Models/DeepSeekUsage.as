// RFC 038: DeepSeek API usage DTO（internal，仅服务 Provider 反序列化）。
namespace Arc.Agent.DeepSeek;
using Arc.Text.Json;

/// <summary>
/// Token 用量统计。
///  JSON: {"prompt_tokens":10,"completion_tokens":20,"total_tokens":30,
///         "prompt_cache_hit_tokens":0,"prompt_cache_miss_tokens":10}
/// </summary>
internal class DeepSeekUsage : IJsonDeserializable {
    public int PromptTokens;
    public int CompletionTokens;
    public int TotalTokens;
    public int PromptCacheHitTokens;
    public int PromptCacheMissTokens;

    public DeepSeekUsage() {
        this.PromptTokens = 0;
        this.CompletionTokens = 0;
        this.TotalTokens = 0;
        this.PromptCacheHitTokens = 0;
        this.PromptCacheMissTokens = 0;
    }

    /// <summary>JSON 反序列化：token 计数与 prompt 缓存命中/写入字段。</summary>
    public void ReadJson(JsonReader reader) {
        while (reader.Read()) {
            if (reader.TokenType == JsonTokenType.EndObject) {
                return;
            }
            if (reader.TokenType == JsonTokenType.PropertyName) {
                string prop = reader.GetString();
                reader.Read();
                if (prop == "prompt_tokens") {
                    this.PromptTokens = reader.GetInt32();
                } else if (prop == "completion_tokens") {
                    this.CompletionTokens = reader.GetInt32();
                } else if (prop == "total_tokens") {
                    this.TotalTokens = reader.GetInt32();
                } else if (prop == "prompt_cache_hit_tokens") {
                    this.PromptCacheHitTokens = reader.GetInt32();
                } else if (prop == "prompt_cache_miss_tokens") {
                    this.PromptCacheMissTokens = reader.GetInt32();
                } else {
                    reader.Skip();
                }
            }
        }
    }
}
