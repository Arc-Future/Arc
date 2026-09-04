// RFC 038: DeepSeek provider configuration DTO.
// Holds API credentials and optional model parameters.
// Note: Arc does not support nullable value types (int?, double?); use sentinel values.
// MaxTokens <= 0 means "not set"; Temperature < 0 means "not set"; TopP < 0 means "not set".
namespace Arc.Agent.DeepSeek;

/// <summary>
/// DeepSeek 模型提供商配置。ApiKey 在构造函数中设置；
/// 其余可选参数为公开字段（可构造后修改）。
/// </summary>
public class DeepSeekOptions {
    /// <summary>DeepSeek API 密钥（必填）。</summary>
    public string ApiKey;

    /// <summary>API 基础 URL，默认 "https://api.deepseek.com"。</summary>
    public string BaseUrl;

    /// <summary>模型标识符，默认 "deepseek-v4-pro"。</summary>
    public string Model;

    /// <summary>最大输出 token 数。≤0 表示不设置。</summary>
    public int MaxTokens;

    /// <summary>采样温度 0–2。&lt;0 表示不设置。</summary>
    public double Temperature;

    /// <summary>核采样 top_p 0–1。&lt;0 表示不设置。</summary>
    public double TopP;

    /// <summary>推理强度 "low" / "high" / "max"（null 或空串表示不设置）。</summary>
    public string ReasoningEffort;

    /// <summary>是否启用思考/推理输出（reasoning_content）。0 = 默认启用（DeepSeek 推理模型惯例）；
    /// -1 = 禁用（不发射 thinking 字段，省延迟/成本）；1 = 显式启用。哨兵语义（Arc 无可空值类型）。</summary>
    public int Thinking;

    /// <summary>创建 DeepSeek 选项（ApiKey 必填）。</summary>
    public DeepSeekOptions(string apiKey) {
        if (apiKey == null) { throw new ArgumentNullException("apiKey"); }
        this.ApiKey = apiKey;
        this.BaseUrl = "https://api.deepseek.com";
        this.Model = "deepseek-v4-pro";
        this.MaxTokens = 0;
        this.Temperature = -1.0;
        this.TopP = -1.0;
        this.Thinking = 0;
    }
}
