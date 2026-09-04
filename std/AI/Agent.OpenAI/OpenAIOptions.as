// RFC 038: OpenAI provider configuration DTO（Arc.Agent.OpenAI）。
//
// 官方端点：Base URL https://api.openai.com/v1（Chat Completions /v1/chat/completions）。
// BaseUrl 必须含 "/v1"（OpenAI 官方契约）；Provider 去尾斜杠后补 "/chat/completions"。
// 哨兵语义（Arc 无可空值类型）：MaxCompletionTokens<=0、Temperature<0、TopP<0 表示不设置。
// ReasoningEffort（"minimal"/"low"/"medium"/"high"）对 o 系列推理模型生效，空串不发射。
namespace Arc.Agent.OpenAI;

/// <summary>
/// OpenAI 模型提供商配置。ApiKey 在构造函数中设置；其余可选参数为公开字段（可构造后修改）。
/// ApiKey 经环境变量注入，禁止硬编码或落盘。
/// </summary>
public class OpenAIOptions {
    /// <summary>OpenAI API 密钥（必填）。</summary>
    public string ApiKey;

    /// <summary>API 基础 URL，默认 "https://api.openai.com/v1"（须含 "/v1"）。</summary>
    public string BaseUrl;

    /// <summary>模型标识符，默认 "gpt-4o-mini"。</summary>
    public string Model;

    /// <summary>最大输出 token 数（OpenAI 官方 max_completion_tokens，o 系列与新模型）。≤0 表示不设置。</summary>
    public int MaxCompletionTokens;

    /// <summary>采样温度 0–2。&lt;0 表示不设置。</summary>
    public double Temperature;

    /// <summary>核采样 top_p 0–1。&lt;0 表示不设置。</summary>
    public double TopP;

    /// <summary>推理强度 "minimal"/"low"/"medium"/"high"（o 系列；null 或空串表示不设置）。</summary>
    public string ReasoningEffort;

    /// <summary>创建 OpenAI 选项（ApiKey 必填）。</summary>
    public OpenAIOptions(string apiKey) {
        if (apiKey == null) { throw new ArgumentNullException("apiKey"); }
        this.ApiKey = apiKey;
        this.BaseUrl = "https://api.openai.com/v1";
        this.Model = "gpt-4o-mini";
        this.MaxCompletionTokens = 0;
        this.Temperature = -1.0;
        this.TopP = -1.0;
        this.ReasoningEffort = "";
    }
}
