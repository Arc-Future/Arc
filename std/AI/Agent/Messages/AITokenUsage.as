namespace Arc.Agent;

/// <summary>
/// 一次模型回复的 token 用量统计（含 prompt 缓存命中/写入，对齐 Anthropic cache_* / DeepSeek prompt_cache_* 字段）。
/// Provider 上报后由 AISession 汇总（TotalUsage）并触发 UsageReported 事件（业务端计费/监控）。
/// </summary>
public class AITokenUsage {
    public int PromptTokens;
    public int CompletionTokens;
    public int TotalTokens;
    /// <summary>prompt 缓存命中输入 token（Anthropic cache_read_input_tokens / DeepSeek prompt_cache_hit_tokens）；0 = 无缓存命中。</summary>
    public int CacheReadTokens;
    /// <summary>prompt 缓存写入输入 token（Anthropic cache_creation_input_tokens / DeepSeek prompt_cache_miss_tokens，即新进入缓存的前缀部分）。</summary>
    public int CacheCreationTokens;

    public AITokenUsage() {
        this.PromptTokens = 0;
        this.CompletionTokens = 0;
        this.TotalTokens = 0;
        this.CacheReadTokens = 0;
        this.CacheCreationTokens = 0;
    }

    /// <summary>是否无任何用量信息（Provider 未上报）。</summary>
    public bool IsEmpty {
        get {
            return this.PromptTokens == 0 && this.CompletionTokens == 0
                && this.TotalTokens == 0 && this.CacheReadTokens == 0
                && this.CacheCreationTokens == 0;
        }
    }
}
