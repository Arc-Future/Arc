// AIModelServiceOptions — 统一服务基座配置（RFC 041 §7.3）。
//
// 单次调用超时 / 幂等重试（指数退避）/ 成本档位（Agent 回合调度用）/ 调用统计。
// 默认不重试（MaxRetries = 0）——重试仅幂等推理（嵌入/OCR 单输入默认允许，指数
// 退避）；TTS 等非幂等保持默认 0。
namespace Arc.AI;

/// <summary>统一服务基座配置（RFC 041 §7.3；构造 <see cref="AIModelService"/> 注入）。</summary>
public class AIModelServiceOptions {
    /// <summary>默认单次调用超时（毫秒）。</summary>
    private const int DefaultTimeoutMs = 30000;

    /// <summary>单次调用超时（毫秒）；0 = 不超时。</summary>
    public int TimeoutMs;

    /// <summary>幂等推理重试次数；默认 0（不重试非幂等，TTS 等保持 0）。</summary>
    public int MaxRetries;

    /// <summary>重试退避基数（毫秒；指数退避 backoff * 2^(attempt-1)）。</summary>
    public int RetryBackoffMs;

    /// <summary>成本档位（Fast/Medium/Slow；Agent 回合调度用）。</summary>
    public AIModelCost CostClass;

    /// <summary>是否记录调用数/延迟到注册表统计（GetStats 可审计）。</summary>
    public bool TrackUsage;

    public AIModelServiceOptions() {
        this.TimeoutMs = AIModelServiceOptions.DefaultTimeoutMs;
        this.MaxRetries = 0;
        this.RetryBackoffMs = 200;
        this.CostClass = AIModelCost.Medium;
        this.TrackUsage = true;
    }

    /// <summary>默认配置（30s 超时 · 不重试 · Medium · 记账）。</summary>
    public static AIModelServiceOptions Default {
        get { return new AIModelServiceOptions(); }
    }
}
