// AgentHost —— 组合根：Provider + 工作区 + 记忆装配（单一组装点，无领域逻辑）。
//
// 职责边界（分层：Host 层）：
//   - CreateProvider：读环境变量 ARC_DEEPSEEK_API_KEY，缺密钥抛错（Agent 真实接入，无假 Provider）。
//   - CreateAsync：装配会话选项（系统指令含工作区边界 + 能力白名单 + 流式 + 知识面注入 +
//     计划门闩受约束能力）、创建 AIHost、并入持久化记忆、注册项目约定上下文源。
//   - 计划门闩（RFC 038 M8.2）：PlanGatedCapabilities 声明 fs.Write/shell.Run 受约束，
//     AIHost 自动装配内置 plan 工具 + Task 层上下文注入 + 调度层写拦截；审批界面在 Repl 层。
// 组合根不触碰工具实现细节（工具由 [AITool] 声明式注册，经 __AIToolHost 装配）。
namespace ArcAgent.Host;
using Arc;
using Arc.Agent;
using Arc.Agent.DeepSeek;
using Arc.Agent.Harness;
using ArcAgent.Context;
using ArcAgent.Workspace;

/// <summary>组合根：装配 Provider、工作区与记忆（唯一组装点，无领域逻辑）。</summary>
public class AgentHost {
    /// <summary>LLM Provider：真实 DeepSeek（RFC 038 删除假 Provider，真实接入）。</summary>
    public static AIChatClient CreateProvider(string apiKey) {
        if (apiKey == null || apiKey == "") {
            throw new ArgumentException("ARC_DEEPSEEK_API_KEY 未设置——Agent 真实接入 DeepSeek，需提供 API 密钥。");
        }
        DeepSeekOptions dopts = new DeepSeekOptions(apiKey);
        return new DeepSeekChatClient(dopts);
    }

    /// <summary>
    /// 装配宿主：会话选项（系统指令含工作区边界 + 能力白名单 + 流式 + 知识面注入 +
    /// 计划门闩 fs.Write/shell.Run 受约束 + quality.Verify）→ AIHost.Create → 并入记忆 →
    /// 注册项目约定上下文源。返回就绪 AIHost。
    /// Harness AIRfc/DoD 由调用方持有 <see cref="AIHarnessSession"/>（RFC 043 终端薄壳）。
    /// </summary>
    public static async Task<AIHost> CreateAsync(
        AIChatClient provider, AgentWorkspace workspace, AgentContext context, CancellationToken cancellationToken) {
        // 1) 先加载记忆（独立 AIWiki）：确定知识面注入路径（WikiPathsToAttach 须在
        //    AIHost.Create 前设定——AIWikiContextProvider 依此注册，并持有 host.Wiki 引用）。
        AIWiki memory = await context.LoadWikiAsync(cancellationToken);

        // 2) 装配会话选项（含计划门闩受约束能力 fs.Write/shell.Run + quality.Verify）。
        AISessionOptions opts = await AgentHost.BuildOptionsAsync(workspace, context, memory, cancellationToken);

        // 3) 创建宿主（host.Wiki 为全新图）→ 并入记忆页。
        //    AIHost.Create 自动识别 PlanGatedCapabilities 非空，装配 AIPlanContextProvider
        //    + 内置 plan/mark_step_done/revise_plan 工具 + 调度层 AIPlanGate 写拦截。
        AIHost host = AIHost.Create(provider, opts);
        AgentHost.MergeMemory(host.Wiki, memory);

        // 4) 注册自定义上下文源：项目约定（.arcagent/conventions.md → Rules 层）。
        host.Context.AddProvider(context.NewConventionsProvider(workspace.Root));
        return host;
    }

    /// <summary>会话选项：系统指令（CodingAgentPrompt 组装）+ 能力白名单 + 流式 + 计划门闩 + 知识面注入。</summary>
    public static async Task<AISessionOptions> BuildOptionsAsync(
        AgentWorkspace workspace, AgentContext context, AIWiki memory, CancellationToken cancellationToken) {
        AISessionOptions opts = new AISessionOptions();
        opts.IsStreaming = true;

        string wsDesc = await workspace.DescribeAsync();
        // 系统提示工程：体系化引导复杂任务执行（身份 + 方法论 + 工具纪律 + 验证纪律 + 输出契约）。
        // planGateEnabled = true：引导模型先计划、批准后才可写入（Reasonix /plan 门闩模式）。
        opts.Instructions = CodingAgentPrompt.Build(wsDesc, true);

        AICapabilitySet caps = new AICapabilitySet();
        caps.Add("ai.Tool");
        caps.Add("fs.Read");
        caps.Add("fs.Write");
        caps.Add("shell.Run");
        // RFC 043：授予 quality.Verify（只读验证工具；不进 PlanGatedCapabilities）。
        // arc_build / arc_test / arc_check / arcgr_query 经 Arc.Agent.Harness.Coding [AITool] 自动装配。
        caps.Add("quality.Verify");
        opts.Capabilities = caps;

        // 计划门闩（RFC 038 M8.2）：声明受约束的写入能力。AIHost 据此自动装配内置
        // plan/mark_step_done/revise_plan 工具 + 调度层拦截——模型先计划、人类批准后写入。
        // quality.Verify 故意不列入——验证工具随时可跑、不进门闩。
        opts.PlanGatedCapabilities = new List<string>();
        opts.PlanGatedCapabilities.Add("fs.Write");
        opts.PlanGatedCapabilities.Add("shell.Run");

        // 知识面注入：记忆知识页经 WikiPathsToAttach 消费桥附到上下文最前（知识库面）。
        List<string> knowledge = context.KnowledgePaths(memory);
        if (knowledge.Count > 0) {
            opts.WikiPathsToAttach = knowledge;
        }
        return opts;
    }

    /// <summary>把记忆页并入目标 Wiki（持久化记忆 → 宿主 Wiki 的搬运）。</summary>
    private static void MergeMemory(AIWiki target, AIWiki memory) {
        if (target == null || memory == null) {
            return;
        }
        foreach (var path in memory.List("")) {
            AIWikiPage page = memory.Get(path);
            if (page != null) {
                target.Upsert(path, page.Body != null ? page.Body : "");
            }
        }
    }
}
