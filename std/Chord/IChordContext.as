// IChordContext —— 和弦上下文契约（RFC 045 D1/D14）。
//
// ChordContext 的 DI 可见面：注册进 ServiceCollection（AddSingleton<IChordContext>），
// 宿主服务经构造注入消费和弦编排面。实现（ChordContext）内部持有 IServiceProvider，
// 类型化服务解析动态阴影链优先、DI 容器兜底（D14）。
namespace Arc.Chord;

/// <summary>
/// 和弦上下文契约——一切操作都发生在上下文上，一切操作都可逆。
/// </summary>
public interface IChordContext : IDisposable {
    // ── 结构信息 ──

    /// <summary>父上下文（根为 null）。</summary>
    ChordContext? Parent { get; }

    /// <summary>作用域（Uid/Name/Status/Config/Error 只读观察）。</summary>
    IScope Scope { get; }

    /// <summary>全局唯一标识。</summary>
    int Uid { get; }

    /// <summary>作用域是否处于 Active。</summary>
    bool IsActive { get; }

    /// <summary>是否已释放。</summary>
    bool IsDisposed { get; }

    /// <summary>当前账本条目数。</summary>
    int EffectCount { get; }

    /// <summary>子上下文数。</summary>
    int ChildCount { get; }

    // ── D2 副作用 ──

    /// <summary>注册副作用：立即执行 callback 并保存撤销句柄（D7）。</summary>
    IDisposable Effect(Func<IDisposable> callback);

    // ── D5 事件 ──

    /// <summary>订阅事件（撤销 = 退订）。</summary>
    IDisposable On(string name, Action<object?> listener);

    /// <summary>订阅事件（prepend 插队到队首）。</summary>
    IDisposable On(string name, Action<object?> listener, bool prepend);

    /// <summary>订阅事件（触发即退订）。</summary>
    IDisposable Once(string name, Action<object?> listener);

    /// <summary>触发事件：自身 + 后代（DFS，快照遍历）。</summary>
    void Emit(string name, object? payload);

    /// <summary>触发事件：自身 + 祖先。</summary>
    void Bubble(string name, object? payload);

    /// <summary>触发事件：仅自身。</summary>
    void EmitSelf(string name, object? payload);

    // ── D5.1 瀑布 ──

    /// <summary>订阅瀑布：handler(payload, next) 按注册序串联，不调 next 即拦截。</summary>
    IDisposable OnWaterfall(string name, Func<object, Func<object, object>, object> handler);

    /// <summary>订阅瀑布（prepend 插队到队首）。</summary>
    IDisposable OnWaterfall(string name, Func<object, Func<object, object>, object> handler, bool prepend);

    /// <summary>触发瀑布：返回末端产出（无订阅时原样返回）。</summary>
    object Waterfall(string name, object payload);

    // ── D3/D14 服务 ──

    /// <summary>提供服务（撤销 = 撤销提供，恢复旧条目）。</summary>
    IDisposable Provide(string name, object? instance);

    /// <summary>类型化提供服务：键 = typeof(T).FullName。</summary>
    IDisposable Provide<T>(T instance) where T : class;

    /// <summary>按工厂提供服务：首次解析时构造并缓存（MEDI 工厂语义同构）。</summary>
    IDisposable Provide<T>(Func<T> factory) where T : class;

    /// <summary>取服务：沿祖先链上溯（本地优先）。</summary>
    object? GetService(string name);

    /// <summary>类型化取服务：动态阴影链优先，DI 容器兜底。</summary>
    T? GetService<T>() where T : class;

    /// <summary>取服务：仅本地。</summary>
    object? GetLocalService(string name);

    /// <summary>服务是否可达（本地或祖先链）。</summary>
    bool HasService(string name);

    /// <summary>类型化服务是否可解析（动态在场或 DI 可解析）。</summary>
    bool HasService<T>() where T : class;

    // ── D2 配置 ──

    /// <summary>设置配置（撤销 = 恢复旧值）。</summary>
    IDisposable SetConfig(string name, object? value);

    /// <summary>取配置：沿祖先链上溯。</summary>
    object? GetConfig(string name);

    /// <summary>配置是否存在（本地或祖先链）。</summary>
    bool HasConfig(string name);

    // ── D4/D14 注入 ──

    /// <summary>依赖注入：全部依赖就绪立即执行，否则挂起等待。</summary>
    IDisposable Inject(string[] names, Action<ChordContext> callback);

    /// <summary>反应式注入：依赖消失回滚回调副作用，重新可用自动重跑。</summary>
    IDisposable InjectReactive(string[] names, Action<ChordContext> callback);

    /// <summary>类型化注入：DI 可解析恒就绪；否则等待动态提供，值直入回调。</summary>
    IDisposable Inject<T>(Action<ChordContext, T?> callback) where T : class;

    /// <summary>类型化反应式注入（动态依赖消失回滚重跑）。</summary>
    IDisposable InjectReactive<T>(Action<ChordContext, T?> callback) where T : class;

    // ── D1/D7/D12 安装音 ──

    /// <summary>安装音（函数形态，无清理需求）。</summary>
    ChordContext Tone(Action<ChordContext> apply);

    /// <summary>安装音（函数形态 + 配置）。</summary>
    ChordContext Tone(Action<ChordContext> apply, object? config);

    /// <summary>安装音（函数形态，返回撤销句柄作为清理动作）。</summary>
    ChordContext Tone(Func<ChordContext, IDisposable> apply);

    /// <summary>安装音（函数形态，返回撤销句柄 + 配置）。</summary>
    ChordContext Tone(Func<ChordContext, IDisposable> apply, object? config);

    /// <summary>安装音（对象形态）。</summary>
    ChordContext Tone(ITone tone);

    /// <summary>安装音（对象形态 + 配置）；实现 IToneRequirements 时按声明准入（D12）。</summary>
    ChordContext Tone(ITone tone, object? config);

    // ── D8 热替换 / D6 事务 ──

    /// <summary>原位热替换：先装新、成功后再卸旧；失败则旧音保持运行并抛出。</summary>
    ChordContext Reload(ChordContext oldContext, Action<ChordContext> apply);

    /// <summary>原位热替换（含配置）：新音插入旧音原位置（保持音序）。</summary>
    ChordContext Reload(ChordContext oldContext, Action<ChordContext> apply, object? config);

    /// <summary>开启副作用事务：Commit 原子合并 / Dispose 回滚。</summary>
    ChordContext BeginTransaction();

    /// <summary>提交事务：效果条目、子上下文、挂起注入原子迁移到父上下文。</summary>
    void Commit();

    // ── D9 生命周期 ──

    /// <summary>注册 ready 钩子（已过阶段立即执行）。</summary>
    void OnReady(Action callback);

    /// <summary>注册 start 钩子（已过阶段立即执行）。</summary>
    void OnStart(Action callback);

    /// <summary>注册 stop 钩子（Stop/Dispose 时逆序执行）。</summary>
    void OnStop(Action callback);

    /// <summary>启动：ready 钩子 → start 钩子 → 级联子上下文。</summary>
    void Start();

    /// <summary>整体卸载：子上下文先于自身释放 → stop 钩子 → 效果 LIFO 撤销。</summary>
    void Stop();
}
