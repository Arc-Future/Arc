// ReviewHost —— 组合根：Provider + 会话选项装配（领域二，无 Coding 依赖）。
namespace ReviewAgent.Host;
using Arc;
using Arc.Agent;
using Arc.Agent.DeepSeek;
using ReviewAgent.Prompt;

/// <summary>组合根：装配 Provider 与会话选项（唯一组装点，无领域逻辑）。</summary>
public class ReviewHost {
    /// <summary>LLM Provider：真实 DeepSeek（与 ArcAgent 同源 Provider 基座）。</summary>
    public static AIChatClient CreateProvider(string apiKey) {
        if (apiKey == null || apiKey == "") {
            throw new ArgumentException("ARC_DEEPSEEK_API_KEY 未设置——ReviewAgent 真实接入 DeepSeek，需提供 API 密钥。");
        }
        DeepSeekOptions dopts = new DeepSeekOptions(apiKey);
        return new DeepSeekChatClient(dopts);
    }

    /// <summary>
    /// 会话选项：领域提示 + 能力白名单（ai.Tool / fs.Read / fs.Write / review.Run）。
    /// review.Run（领域只读工具）不进计划门闩——随时可跑；fs.Write 受约束（评审报告落盘先批准计划）。
    /// 领域 [AITool] 经编译期贡献自动装配（AIHost 全局默认工具源），此处只开能力白名单。
    /// </summary>
    public static AISessionOptions BuildOptions() {
        AISessionOptions opts = new AISessionOptions();
        opts.IsStreaming = true;
        opts.Instructions = ReviewAgentPrompt.Build();
        AICapabilitySet caps = new AICapabilitySet();
        caps.Add("ai.Tool");
        caps.Add("fs.Read");
        caps.Add("fs.Write");
        caps.Add("review.Run");
        opts.Capabilities = caps;
        opts.PlanGatedCapabilities = new List<string>();
        opts.PlanGatedCapabilities.Add("fs.Write");
        return opts;
    }
}
