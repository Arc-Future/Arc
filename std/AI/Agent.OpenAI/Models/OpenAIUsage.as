// RFC 038: OpenAI API usage DTO（public：见 OpenAIResponse 挂账说明）。
namespace Arc.Agent.OpenAI;
using Arc.Text.Json;

/// <summary>
/// Token 用量统计（OpenAI 官方字段）。
///  JSON: {"prompt_tokens":10,"completion_tokens":20,"total_tokens":30,
///         "prompt_tokens_details":{"cached_tokens":4},
///         "completion_tokens_details":{"reasoning_tokens":8}}
/// prompt_tokens_details.cached_tokens（prompt 缓存命中）映射 AITokenUsage.CacheReadTokens。
/// completion_tokens_details.reasoning_tokens（o 系列推理 token 计数）在冻结的 AITokenUsage
/// 无对应槽，诚实跳过、不伪造映射。
/// </summary>
public class OpenAIUsage : IJsonDeserializable {
    public int PromptTokens;
    public int CompletionTokens;
    public int TotalTokens;
    public int CachedTokens;

    public OpenAIUsage() {
        this.PromptTokens = 0;
        this.CompletionTokens = 0;
        this.TotalTokens = 0;
        this.CachedTokens = 0;
    }

    /// <summary>JSON 反序列化：token 计数与 prompt_tokens_details.cached_tokens。</summary>
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
                } else if (prop == "prompt_tokens_details") {
                    this.ReadPromptTokensDetails(reader);
                } else {
                    reader.Skip();
                }
            }
        }
    }

    /// <summary>读取 prompt_tokens_details 对象：取 cached_tokens（prompt 缓存命中）。</summary>
    private void ReadPromptTokensDetails(JsonReader reader) {
        while (reader.Read()) {
            if (reader.TokenType == JsonTokenType.EndObject) {
                return;
            }
            if (reader.TokenType == JsonTokenType.PropertyName) {
                string prop = reader.GetString();
                reader.Read();
                if (prop == "cached_tokens") {
                    this.CachedTokens = reader.GetInt32();
                } else {
                    reader.Skip();
                }
            }
        }
    }
}
