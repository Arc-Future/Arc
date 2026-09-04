// RFC 038 上下文成体系：AIContextProvider — 上下文源扩展契约
//（MAF contract-first，抽象基类，双向管道）。
//
// Agent = LLM + Context + Tools。本类型是「Context」一轴的扩展点，吸收微软 MAF 的
// contract-first 设计精华（五要素）+ 双向管道（RFC 038）：
//   ① 契约封闭：只暴露契约类型（AIContextQuery / AIContextBlock / AIContextSession /
//      AIContextHost），宿主（组合根）与 provider 互不依赖对方实现，可独立演进 / 版本化。
//   ② 身份与元数据：Name / Description / Version / Priority —— 审计、去重与布局归属
//      （对应 MAF AddInToken）。
//   ③ 生命周期：Initialize(host) / Deactivate() / Dispose() —— 显式启动与停用
//      （对应 MAF Activate / Deactivate / Shutdown）。
//   ④ 宿主环境：Initialize 注入 AIContextHost（对应 AddInEnvironment /
//      IServiceProviderContract），provider 经 Host 访问宿主共享服务（Wiki / token 预算）。
//   ⑤ 双向管道：调用前 ProvideContextAsync(query, session, ct) 注入上下文块（支撑
//      RAG / DB / 网络等真实动态源；静态源返回已完成 Task）；调用后
//      ProcessMessageAsync(message, session, ct) 抽取 / 持久化（记忆 / Wiki / Skill 落库）。
//      provider 实例跨会话共享（Host 级），会话态一律经 AIContextSession /
//      AIProviderSessionState<TState> 按名读写，实例零会话态字段（RFC 038）。
//
// 实现形态：抽象基类而非纯 interface——Arc 编译器对「跨程序集接口分派」存在已知缺口
//（宿主程序集经接口回调用户程序集实现会崩溃；同程序集正常）。框架既有跨集扩展正道
// 即抽象基类（AIChatClient / AIToolHandler），本契约沿用同一惯用法，保持契约面稳定。
//
// 诚实边界：本契约只定义「上下文源」，不定义编排 / 检索细节（RAG 检索编排为 App 侧
// 非目标边界，RFC 004/028 保持）。
namespace Arc.Agent;
using Arc.Collections;
using Arc;

/// <summary>
/// 可插拔上下文源契约（抽象基类，MAF contract-first 五要素 + 双向管道）。实现方在
/// 模型往返前经 <see cref="ProvideContextAsync"/> 产出自描述 <see cref="AIContextBlock"/>，
/// 往返后经 <see cref="ProcessMessageAsync"/> 抽取 / 持久化会话态。
/// <see cref="AIContextEngine"/>（Host 级组合根）按注册序收集、预算裁剪、按
/// (Kind 固定序 → Priority) 稳定排序、扁平化合并为请求消息面。provider 实例跨会话共享
/// （Host 级注册，RFC 038）；会话态一律经 <see cref="AIContextSession"/> /
/// <see cref="AIProviderSessionState{TState}"/> 按名读写，实例字段零会话态。
/// 开发者自定义源（RAG 检索 / 用户画像 / 时间感知 / 记忆等）继承本类并 AddProvider
/// 挂入 Host，无需改动引擎。
/// </summary>
public abstract class AIContextProvider : IDisposable {
    /// <summary>宿主环境（MAF AddInEnvironment；Initialize 注入；Activate 前为 null）。</summary>
    private AIContextHost _host;

    // ── ② 身份与元数据（MAF AddInToken） ──

    /// <summary>上下文源唯一名（MAF 身份：审计 / 去重 / 顺序归属）。</summary>
    public abstract string GetName();

    /// <summary>人类可读描述（审计 / 文档 / 发现）。</summary>
    public virtual string GetDescription() {
        return "";
    }

    /// <summary>源版本（独立演进 / 兼容判断）。</summary>
    public virtual string GetVersion() {
        return "1.0";
    }

    /// <summary>缺省布局优先级（小值靠前；可被块级 Priority 覆盖）。</summary>
    public virtual int GetPriority() {
        return 0;
    }

    // ── ④ 宿主环境（MAF AddInEnvironment / IServiceProviderContract） ──

    /// <summary>宿主共享环境（Wiki / token 预算；Initialize 后非空）。</summary>
    public AIContextHost Host {
        get { return _host; }
    }

    // ── ③ 生命周期（MAF Activate / Deactivate / Dispose） ──

    /// <summary>激活：注入宿主环境句柄（组合根在首轮构建前调用）。派生可覆写以启动资源。</summary>
    public virtual void Initialize(AIContextHost host) {
        _host = host;
    }

    /// <summary>停用：释放源私有的长生命周期资源（组合根在释放引擎时调用）。</summary>
    public virtual void Deactivate() {
    }

    /// <summary>释放：默认停用（组合根统一释放全部源）。</summary>
    public void Dispose() {
        this.Deactivate();
    }

    // ── ① 双向管道 ──

    /// <summary>
    /// 调用前方向：构建本源的全部上下文块（空 = 无贡献；顺序、优先级须稳定）。查询感知：
    /// RAG 等动态源据 <paramref name="query"/> 按需检索；静态源忽略 query，返回已完成 Task。
    /// <paramref name="session"/> 为本会话态载体（跨会话共享实例下按名读写自身的会话态）。
    /// </summary>
    public abstract Task<List<AIContextBlock>> ProvideContextAsync(AIContextQuery query, AIContextSession session, CancellationToken cancellationToken);

    /// <summary>
    /// 调用后方向：模型往返后把刚追加的消息抽取 / 持久化（记忆 / Wiki / Skill 落库类源覆写）。
    /// 默认空实现（不消费消息）。单源异常由组合根容错，不打断会话回合。
    /// </summary>
    public virtual Task ProcessMessageAsync(AIMessage message, AIContextSession session, CancellationToken cancellationToken) {
        return Task.CompletedTask;
    }
}