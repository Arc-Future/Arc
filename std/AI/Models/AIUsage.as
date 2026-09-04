// RFC 041 §7.5：AIUsage — OpenAI usage 对齐 + 本地显式扩展。
//
// 对齐 OpenAI usage 对象（prompt_tokens / completion_tokens / total_tokens）；
// 本地扩展 DurationMs / PeakMemoryBytes 为显式扩展字段，不冒充 OpenAI 参数进请求体。
// Arc 无可空值类型（int?/long?）——未上报以 -1 哨兵表示（对齐 DeepSeekOptions 先例）。
namespace Arc.AI.Models;

/// <summary>模型调用用量（RFC 041 §7.5）。所有计数/扩展字段 -1 = 模型未上报。</summary>
public class AIUsage {
    /// <summary>提示 token 数（-1 = 未上报）。</summary>
    public int PromptTokens { get; set; }

    /// <summary>补全 token 数（-1 = 未上报）。</summary>
    public int CompletionTokens { get; set; }

    /// <summary>总 token 数（-1 = 未上报）。</summary>
    public int TotalTokens { get; set; }

    /// <summary>本地扩展：本次调用耗时（毫秒；-1 = 未采集）。</summary>
    public long DurationMs { get; set; }

    /// <summary>本地扩展：峰值内存（字节；-1 = 未采集）。</summary>
    public long PeakMemoryBytes { get; set; }

    public AIUsage() {
        this.PromptTokens = -1;
        this.CompletionTokens = -1;
        this.TotalTokens = -1;
        this.DurationMs = -1;
        this.PeakMemoryBytes = -1;
    }
}
