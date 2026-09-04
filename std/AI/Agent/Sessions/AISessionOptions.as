namespace Arc.Agent;
using Arc.Collections;
public class AISessionOptions {
    public int MaxTurns;
    /// <summary>消息条数上限（缓存安全兜底：默认 128 条不触发裁剪；见 WindowKeepLast 缓存语义）。</summary>
    public int MaxMessages;
    /// <summary>
    /// 附加到会话上下文的 Wiki 路径（消费桥：AIWiki 只写不读的封口）。
    /// BuildRequest 时把对应 AIWiki 页以 system 上下文注入消息面最前（知识库面）。
    /// 空 = 不注入。注意：注入内容随页变更而变化 → 可能打破 KV Cache 前缀稳定，仅在确实
    /// 需要外部记忆时启用（对齐 Instructions 的缓存语义权衡）。
    /// </summary>
    public List<string> WikiPathsToAttach;
    /// <summary>
    /// 窗口裁剪策略（RFC 038）：0 = 关闭（默认，请求面 = 完整 append-only 历史 → 前缀
    /// 完整匹配已落盘的缓存前缀单元 → 命中 LLM 上下文缓存，对齐 DeepSeek KV Cache）；
    /// &gt;0 = BuildRequest 前保留系统消息 + 最近 K 条——**注意：裁剪切断中间消息 → 后续请求
    /// 无法完整匹配缓存前缀单元 → 缓存失效**，仅在窗口必须受限时启用。
    /// </summary>
    public int WindowKeepLast;
    /// <summary>工具循环护栏（RFC 038）：同工具同签名连续轮次超限 → Failed("ToolLoopGuard")；0 = 关闭。</summary>
    public int MaxConsecutiveIdenticalToolRounds;
    /// <summary>
    /// 上下文 token 预算上限（RFC 038）：0 = 不设限（默认）。&gt;0 = AIContextEngine 按
    /// 块级 TokenEstimate 从尾部裁剪超限上下文块（DroppedBlocks 可审计）。对齐 LLM 模型窗口
    /// 安全兜底，不打破前缀稳定（仅丢尾部低优先块）。
    /// </summary>
    public int MaxContextTokens;
    /// <summary>
    /// Instructions（系统指令）：首轮注入为最前 system 消息，此后保持稳定（空 = 不注入）。
    /// 上下文完整性对齐 DeepSeek 上下文硬盘缓存（KV Cache，api-docs.deepseek.com/guides/kv_cache）：
    /// 命中前提是请求**完整匹配**已落盘的「缓存前缀单元」——Instructions 置于最前且字节不变
    /// + 消息面 append-only 不裁剪 → 每轮请求完整复用上一轮前缀 → 命中缓存
    /// （usage.prompt_cache_hit_tokens 反映命中数）。窗口裁剪（WindowKeepLast &gt; 0）会切断
    /// 前缀单元 → 缓存失效，仅窗口必须受限时启用。
    /// </summary>
    public string Instructions;
    public AIToolStreamHandler ToolStreamHandler;
    /// <summary>Concrete TakeOver handler for stream pumps (abstract virtual mid-stream is unreliable).</summary>
    public AITakeOverStreamHandler TakeOverHandler;
    public AIToolSet Tools;
    /// <summary>
    /// 挂载的 Skill 能力单元（上下文工程基座消费）。激活 Skill 的激活提示注入 system
    /// 上下文，其能力工具并入请求 tools 数组。空 = 无 Skill。
    /// </summary>
    public AISkillSet Skills;
    public AICapabilitySet Capabilities;
    /// <summary>
    /// 可选依赖注入容器（IServiceProvider，ARC DI 架构化）。由 AIHost 一并透传至会话，
    /// 供工具/上下文/业务服务在会话生命周期内按需解析。null = 不启用 DI。
    /// </summary>
    public IServiceProvider Services;
    /// <summary>
    /// 响应格式契约（MAF contract-first；null = 不约束）。由 AIHost 设置，透传到每轮请求；
    /// Provider 按自身 API 协议内部映射（DeepSeek：json_object / json_schema）。可用
    /// AIResponseFormat.From&lt;T&gt;() 从类型结构声明式获得严格 Schema。
    /// </summary>
    public AIResponseFormat ResponseFormat;
    public bool IsStreaming;
    /// <summary>HITL 策略：true 时所有工具执行前都需人机确认（RFC 038）。</summary>
    public bool ApproveAllTools;
    /// <summary>
    /// 受计划门闩约束的能力白名单（如 fs.Write / shell.Run）；非空 = 启用计划门闩。
    /// 启用后 AIHost 自动装配 AIPlanContextProvider（Task 层上下文注入）+ 内置
    /// plan / mark_step_done / revise_plan 工具（能力 ai.Plan），并在调度层按本名单
    /// 拦截「存在未批准计划」时的写入能力。只读能力 / 无计划（简单任务）一律放行。
    /// 空 = 不启用（默认）。
    /// </summary>
    public List<string> PlanGatedCapabilities;
    public AISessionOptions() {
        this.MaxTurns = 16;
        this.MaxMessages = 128;
        this.WindowKeepLast = 0;
        this.WikiPathsToAttach = new List<string>();
        this.MaxConsecutiveIdenticalToolRounds = 3;
        this.MaxContextTokens = 0;
        this.Instructions = "";
        this.ToolStreamHandler = null;
        this.TakeOverHandler = null;
        this.Tools = null;
        this.Skills = new AISkillSet();
        this.Capabilities = null;
        this.Services = null;
        this.ResponseFormat = null;
        this.IsStreaming = false;
        this.ApproveAllTools = false;
        this.PlanGatedCapabilities = new List<string>();
    }
    public static AISessionOptions Default { get { return new AISessionOptions(); } }

    /// <summary>深拷贝选项（新实例；不改写调用方对象）。Skill 注册表深拷贝；其余引用字段浅拷贝。</summary>
    public AISessionOptions Clone() {
        AISessionOptions c = new AISessionOptions();
        c.MaxTurns = this.MaxTurns;
        c.MaxMessages = this.MaxMessages;
        c.WindowKeepLast = this.WindowKeepLast;
        c.MaxConsecutiveIdenticalToolRounds = this.MaxConsecutiveIdenticalToolRounds;
        c.MaxContextTokens = this.MaxContextTokens;
        c.Instructions = this.Instructions;
        c.ToolStreamHandler = this.ToolStreamHandler;
        c.TakeOverHandler = this.TakeOverHandler;
        c.Tools = this.Tools;
        c.Skills = this.Skills != null ? this.Skills.Clone() : new AISkillSet();
        c.Capabilities = this.Capabilities;
        c.Services = this.Services;
        c.ResponseFormat = this.ResponseFormat;
        c.IsStreaming = this.IsStreaming;
        c.ApproveAllTools = this.ApproveAllTools;
        if (this.PlanGatedCapabilities != null) {
            int p = 0;
            int pn = this.PlanGatedCapabilities.Count;
            while (p < pn) {
                c.PlanGatedCapabilities.Add(this.PlanGatedCapabilities[p]);
                p = p + 1;
            }
        }
        if (this.WikiPathsToAttach != null) {
            int i = 0;
            int n = this.WikiPathsToAttach.Count;
            while (i < n) {
                c.WikiPathsToAttach.Add(this.WikiPathsToAttach[i]);
                i = i + 1;
            }
        }
        return c;
    }
}
