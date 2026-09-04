namespace Arc.Agent;
using Arc;
/// <summary>
/// 唯一宿主入口。绑 Provider + 宿主级 Options（AISessionOptions，单一事实源） + Wiki；禁空桩。
/// 宿主级选项在 CreateSession 时向会话提供工具/能力/流式 handler 默认（会话可覆盖）。
/// RFC 038：宿主不再使用独立 AIHostOptions——统一以 AISessionOptions 为唯一选项类型，消除字段重复。
/// </summary>
public class AIHost : IDisposable {
    private AIChatClient _provider;
    public AISessionOptions Options;
    public AIWiki Wiki;
    private AICoordinator _coordinator;
    // RFC 038：Host 级共享 Context 组合根（跨会话复用，provider 实例一次注册）。
    private AIContextEngine _contextEngine;
    // 计划门闩（RFC 038 M8.2）：Options.PlanGatedCapabilities 非空时启用；持有当前计划
    // provider + 受约束能力集合 + 审批事件；经 CreateSession 注入 sandbox 做调度层拦截。
    private AIPlanGate _planGate;
    private bool _disposed;

    /// <summary>
    /// 全局默认工具源（RFC 038）：编译器注入 __AIToolHost.__RegisterGlobal 在程序入口注册，
    /// 实例化 AIHost 即自动获得全部 [AITool] 工具。用户显式 Tools 优先于它；
    /// 真实生效仍由 AICapabilitySet 白名单 fail-closed 授权。
    /// </summary>
    private static Func<IServiceProvider, AIToolSet> _defaultToolSource = null;

    private AIHost() {
        _provider = null;
        this.Options = null;
        this.Wiki = null;
        _coordinator = null;
        _contextEngine = null;
        _planGate = null;
        _disposed = false;
    }

    /// <summary>注册全局默认工具源（框架内部；编译器引导调用，非用户特性）。</summary>
    public static void SetDefaultToolSource(Func<IServiceProvider, AIToolSet> source) {
        _defaultToolSource = source;
    }

    public IAIChatClient Provider {
        get { return _provider; }
    }

    /// <summary>宿主级写协调器（M7 多会话冲突规避）：跨会话写意图登记 / 冲突检测 / 原子提交。</summary>
    public AICoordinator Coordinator {
        get { return _coordinator; }
    }

    /// <summary>
    /// Host 级共享 Context 组合根（RFC 038）：跨会话复用；开发者可经
    /// Context.AddProvider(...) 注入 / 替换 / 移除自定义上下文源。内置
    /// Instructions/Skill/Wiki 已作为普通 provider 注册（RFC 038 收编）。
    /// </summary>
    public AIContextEngine Context {
        get { return _contextEngine; }
    }

    /// <summary>
    /// 计划门闩（RFC 038 M8.2）：Options.PlanGatedCapabilities 非空时非 null。
    /// 应用侧经其读取当前计划、批准/拒绝（Approve/Reject）、订阅生命周期事件（SetEvents）。
    /// </summary>
    public AIPlanGate PlanGate {
        get { return _planGate; }
    }

    public static AIHost Create(AIChatClient provider) {
        return AIHost.Create(provider, AISessionOptions.Default);
    }

    public static AIHost Create(AIChatClient provider, AISessionOptions options) {
        if (provider == null) {
            throw new ArgumentNullException("provider");
        }
        AIHost host = new AIHost();
        host._provider = provider;
        if (options != null) {
            host.Options = options;
        } else {
            host.Options = AISessionOptions.Default;
        }
        host.Wiki = new AIWiki();
        host._coordinator = new AICoordinator();
        // RFC 038：构建 Host 级共享 Context 组合根，并把内置 Instructions/Skill/Wiki
        // 收编为普通 provider 注册（顺序：Instructions → Skill → Wiki，保持前缀稳定）。
        // 空源不注册（无贡献）；全部经 AddProvider 走统一 provider 管道，无硬编码特判。
        host._contextEngine = new AIContextEngine(host.Options.Tools, host.Options.Skills, host.Wiki, host.Options.MaxContextTokens);
        if (host.Options.Instructions != null && host.Options.Instructions != "") {
            host._contextEngine.AddProvider(new AIInstructionContextProvider(host.Options.Instructions));
        }
        if (host.Options.Skills != null && host.Options.Skills.Count > 0) {
            host._contextEngine.AddProvider(new AISkillContextProvider(host.Options.Skills));
        }
        if (host.Options.WikiPathsToAttach != null && host.Options.WikiPathsToAttach.Count > 0) {
            host._contextEngine.AddProvider(new AIWikiContextProvider(host.Options.WikiPathsToAttach, host.Wiki));
        }
        // 计划门闩装配（RFC 038 M8.2）：PlanGatedCapabilities 非空即启用——创建宿主级
        // 计划 gate + 注册 AIPlanContextProvider（Task 层注入）。内置计划工具与 sandbox
        // 拦截在 CreateSession 按会话装配（tools 动态解析后）。空 = 不启用（默认）。
        AISessionOptions popts = host.Options;
        if (popts != null && popts.PlanGatedCapabilities != null && popts.PlanGatedCapabilities.Count > 0) {
            AIPlanContextProvider planProvider = new AIPlanContextProvider();
            host._contextEngine.AddProvider(planProvider);
            AIPlanGate gate = new AIPlanGate();
            gate.Attach(planProvider);
            gate.SetGatedCapabilities(popts.PlanGatedCapabilities);
            host._planGate = gate;
        }
        return host;
    }

    /// <summary>RFC 038 草图签名：Host 同时绑定工具集。</summary>
    public static AIHost Create(AIChatClient provider, AIToolSet tools) {
        if (provider == null) {
            throw new ArgumentNullException("provider");
        }
        AISessionOptions opts = new AISessionOptions();
        opts.Tools = tools;
        return AIHost.Create(provider, opts);
    }

    /// <summary>RFC 038 草图签名：Host 同时绑定工具集与选项（options 只读拷贝，不原地改写）。</summary>
    public static AIHost Create(AIChatClient provider, AIToolSet tools, AISessionOptions options) {
        if (provider == null) {
            throw new ArgumentNullException("provider");
        }
        AISessionOptions opts = new AISessionOptions();
        if (options != null) {
            opts.ToolStreamHandler = options.ToolStreamHandler;
            opts.TakeOverHandler = options.TakeOverHandler;
            opts.Capabilities = options.Capabilities;
        }
        opts.Tools = tools;
        return AIHost.Create(provider, opts);
    }

    public AISession CreateSession() {
        return this.CreateSession(AISessionOptions.Default);
    }

    public AISession CreateSession(AISessionOptions options) {
        this.ThrowIfDisposed();
        // 会话选项 = 调用方选项的副本：CreateSession 不原地改写调用方 options
        //（与 Create(provider, tools, options)「options 只读拷贝」原则一致）。
        AISessionOptions opts = options != null ? options.Clone() : AISessionOptions.Default;
        // Skill 回退：会话未显式携带 Skill 时继承宿主级 Skills（与 Tools/Caps 回退一致）。
        if ((opts.Skills == null || opts.Skills.Count == 0) && this.Options != null && this.Options.Skills != null && this.Options.Skills.Count > 0) {
            opts.Skills = this.Options.Skills.Clone();
        }
        AIToolStreamHandler handler = opts.ToolStreamHandler;
        if (handler == null) {
            handler = this.Options.ToolStreamHandler;
        }
        AITakeOverStreamHandler takeOver = opts.TakeOverHandler;
        if (takeOver == null) {
            takeOver = this.Options.TakeOverHandler;
        }
        if (handler == null && takeOver != null) {
            handler = takeOver;
        }
        AIToolSet tools = opts.Tools;
        if (tools == null) {
            tools = this.Options.Tools;
        }
        // RFC 038：用户未显式绑定工具集时，回退到编译器注册的全局默认工具源
        //（实例化 AIHost 即自动获得全部 [AITool] 工具；真实生效仍靠 capability 白名单）。
        // 先载入局部再 Invoke——静态 delegate 字段直接 `.Invoke()` 会走错误的直接调用路径
        //（`@AIHost__defaultToolSource` 而非静态字段 load），故必须先读到局部变量。
        if (tools == null && AIHost._defaultToolSource != null) {
            Func<IServiceProvider, AIToolSet> src = AIHost._defaultToolSource;
            IServiceProvider svc = opts.Services != null ? opts.Services : this.Options.Services;
            tools = src.Invoke(svc);
        }
        AICapabilitySet caps = opts.Capabilities;
        if (caps == null) {
            caps = this.Options.Capabilities;
        }
        // 响应格式契约回退：会话未显式携带时继承宿主级 ResponseFormat（与 Tools/Caps 回退一致）。
        if (opts.ResponseFormat == null && this.Options != null) {
            opts.ResponseFormat = this.Options.ResponseFormat;
        }
        AIToolSandbox sandbox = null;
        if (tools != null) {
            if (caps == null) {
                // 默认授权 `ai.Tool`（工具缺省能力，见 AIToolSandbox.ExecuteAsync）。
                // 避免 `Create(provider, tools)` 未配 Capabilities 时空白名单全拒；
                // 特殊能力（如 fs.Write）仍须显式 AICapabilitySet 授权，fail-closed 保持。
                caps = AICapabilitySet.From("ai.Tool");
            }
            if (_planGate != null) {
                // 计划门闩启用：内置 plan 工具进工具集 + 自动授权 ai.Plan（内部机制，
                // 非用户特性；用户只需声明 PlanGatedCapabilities 即获得完整计划能力）。
                _planGate.InstallTools(tools);
                caps.Add("ai.Plan");
            }
            sandbox = new AIToolSandbox(tools, caps, takeOver);
            if (_planGate != null) {
                // 调度层门闩：sandbox 按 gate 判定受约束能力的写拦截（未批准计划 → 拒绝）。
                sandbox.PlanGate = _planGate;
            }
            handler = sandbox;
            // 挂载 Skill 工具为外部能力源（Skill 工具并入发射 schema，且可经 sandbox 调度执行）。
            AIToolSet skillTools = opts.Skills != null ? opts.Skills.ToToolSet() : new AIToolSet();
            if (skillTools.Count > 0) {
                sandbox.AttachExternalTools(skillTools);
            }
        }
        // Keep Tools on SessionOptions when Host binds them (Session.Tools reads options).
        if (tools != null && opts.Tools == null) {
            opts.Tools = tools;
        }
        if (caps != null && opts.Capabilities == null) {
            opts.Capabilities = caps;
        }
        return new AISession(_provider, opts, handler, this.Wiki, sandbox, takeOver, _contextEngine);
    }

    /// <summary>
    /// M8 长时任务基座（RFC 038 §3.4）：宿主级工厂创建 <see cref="AITaskRun"/>。
    /// 内部经 CreateSession 派生承载会话，宿主统一持有协调器（Coordinator）供长时任务
    /// 写文件冲突规避（M7）。maxSteps 为单任务有界回合上限。
    /// </summary>
    public AITaskRun CreateTaskRun(int maxSteps) {
        return this.CreateTaskRun(AISessionOptions.Default, maxSteps);
    }

    public AITaskRun CreateTaskRun(AISessionOptions options, int maxSteps) {
        this.ThrowIfDisposed();
        AISession session = this.CreateSession(options);
        return new AITaskRun(session, maxSteps);
    }

    /// <summary>
    /// 创建 CodeAct 门面（无宿主副作用，RFC 038 §7）：不触碰宿主共享 Options，避免污染后续
    /// 会话的能力/工具面。返回门面供配置超时/输出上限；直接调用须具备
    /// <see cref="AICodeAct.CodeActCapability"/>（fail-closed）。如需以工具形态经沙箱调度，
    /// 用 <see cref="CreateCodeAct(IAICodeActProvider, AIToolSet, AICapabilitySet)"/> 显式装配到
    /// 目标会话的作用域 AIToolSet/CapabilitySet。
    /// </summary>
    public AICodeAct CreateCodeAct(IAICodeActProvider provider) {
        this.ThrowIfDisposed();
        if (provider == null) {
            throw new ArgumentNullException("provider");
        }
        return new AICodeAct(provider, AICapabilitySet.From(AICodeAct.CodeActCapability));
    }

    /// <summary>
    /// 显式装配 CodeAct（RFC 038 §7）：把内置 <c>codeact</c> 工具注册进 <paramref name="tools"/>
    /// 并授予 <see cref="AICodeAct.CodeActCapability"/> 进 <paramref name="capabilities"/>。
    /// 两者均为调用方自持对象（按会话/任务作用域），不触碰宿主共享 Options——避免能力/工具面
    /// 污染后续会话。后续会话内经 <see cref="AIToolSandbox"/> 统一走 capability 分派与 HITL
    /// 门闩。返回门面供配置超时/输出上限。
    /// </summary>
    public AICodeAct CreateCodeAct(IAICodeActProvider provider, AIToolSet tools, AICapabilitySet capabilities) {
        this.ThrowIfDisposed();
        if (provider == null) {
            throw new ArgumentNullException("provider");
        }
        if (tools == null) {
            throw new ArgumentNullException("tools");
        }
        if (capabilities == null) {
            throw new ArgumentNullException("capabilities");
        }
        AICodeAct codeAct = new AICodeAct(provider, AICapabilitySet.From(AICodeAct.CodeActCapability));
        AIToolDescriptor desc = new AIToolDescriptor(
            "codeact",
            "在沙箱内执行模型生成的动作代码（须授权 " + AICodeAct.CodeActCapability + "）",
            AICodeAct.CodeActCapability,
            false);
        desc.ParametersSchema = "{\"type\":\"object\",\"properties\":{\"code\":{\"type\":\"string\",\"description\":\"要执行的代码文本\"}},\"required\":[\"code\"]}";
        tools.Add(desc, new CodeActToolHandler(codeAct));
        capabilities.Add(AICodeAct.CodeActCapability);
        return codeAct;
    }

    public void Dispose() {
        if (_disposed) { return; }
        _disposed = true;
        if (_contextEngine != null) {
            _contextEngine.Dispose();
            _contextEngine = null;
        }
    }

    private void ThrowIfDisposed() {
        if (_disposed) {
            throw new ObjectDisposedException("AIHost");
        }
    }
}