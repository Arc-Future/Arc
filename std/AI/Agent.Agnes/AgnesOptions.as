// RFC 038: Agnes provider configuration DTO（Arc.Agent.Agnes）。
//
// 官方端点：Base URL https://apihub.agnes-ai.com/v1（OpenAI 兼容 /v1/chat/completions）；
// 模型 agnes-2.0-flash（免费）/ agnes-2.5-pro（付费 reasoning）。
// 哨兵语义（Arc 无可空值类型）：MaxTokens<=0、Temperature<0、TopP<0 表示不设置。
namespace Arc.Agent.Agnes;

/// <summary>
/// Agnes 模型提供商配置。ApiKey 在构造函数中设置；
/// 其余可选参数为公开字段（可构造后修改）。
/// </summary>
public class AgnesOptions
{
    /// <summary>Agnes API 密钥（必填）。经环境变量注入，禁止硬编码或落盘。</summary>
    public string ApiKey;

    /// <summary>API 基础 URL，默认 "https://apihub.agnes-ai.com/v1"。</summary>
    public string BaseUrl;

    /// <summary>模型标识符，默认 "agnes-2.0-flash"。</summary>
    public string Model;

    /// <summary>最大输出 token 数。≤0 表示不设置。</summary>
    public int MaxTokens;

    /// <summary>采样温度 0–2。&lt;0 表示不设置。</summary>
    public double Temperature;

    /// <summary>核采样 top_p 0–1。&lt;0 表示不设置。</summary>
    public double TopP;

    /// <summary>推理强度（"low" / "medium" / "high"；null 或空串表示不设置）。</summary>
    public string ReasoningEffort;

    /// <summary>是否启用推理输出：0 = 默认（推理模型惯例）；-1 = 禁用（不发射 thinking 字段）；
    /// 1 = 显式启用。哨兵语义（Arc 无可空值类型）。</summary>
    public int Thinking;

    /// <summary>工具选择约束（OpenAI 兼容 tool_choice 原始 JSON；空串 = 不发射）。</summary>
    public string ToolChoice;

    /// <summary>Anthropic 兼容 chat_template_kwargs 原始 JSON（空串 = 不发射）。</summary>
    public string ChatTemplateKwargs;

    /// <summary>创建 Agnes 选项（ApiKey 必填）。</summary>
    public AgnesOptions(string apiKey)
    {
        if (apiKey == null)
        {
            throw new ArgumentNullException("apiKey");
        }
        this.ApiKey = apiKey;
        this.BaseUrl = "https://apihub.agnes-ai.com/v1";
        this.Model = "agnes-2.0-flash";
        this.MaxTokens = 0;
        this.Temperature = -1.0;
        this.TopP = -1.0;
        this.ReasoningEffort = "";
        this.Thinking = 0;
        this.ToolChoice = "";
        this.ChatTemplateKwargs = "";
    }
}
