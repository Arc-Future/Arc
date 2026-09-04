// ChordContext —— 和弦内核唯一操作入口（RFC 045 D1–D12/D14）。
//
// 一切操作都发生在上下文上，一切操作都可逆：Effect/On/Provide/SetConfig/
// Timeout/Interval/Tone/Inject/Contribute 全部实现为副作用账本条目（D2），
// 释放按 LIFO 逆序撤销。安装音即创建子上下文（D1 上下文树）；
// Provide 唤醒子树内挂起注入（D4）与依赖准入音（D12）；事务（D6）经
// 效果迁移原子合并；Reload（D8）先装新、成功后再卸旧。
//
// DI 融合（D14）：上下文可持有 IServiceProvider（经 AddSingleton<IChordContext>
// 注册进 ServiceCollection），类型化服务解析动态阴影链优先、DI 容器兜底；
// DI 可解析的注入依赖恒就绪（静态层不可变，无挂起语义）。
//
// 线程模型：内核单线程同步（D10）——状态机操作须由使用者回送主线程。
namespace Arc.Chord;

using Arc;
using Arc.Collections;
using Arc.DI;
using Arc.Reflection;


public class ChordContext : IChordContext {
    private static int _nextUid = 1;

    internal ChordContext? _parent;
    private List<ChordContext> _children;
    private int _uid;
    private Scope _scope;
    internal EffectRegistry _effects;
    private ServiceRegistry _services;
    private EventEmitter _events;
    private ConfigStore _config;
    private WaterfallRegistry _waterfalls;
    internal List<PendingInjection> _pending;
    private List<PendingTone> _pendingTones;
    private List<Action> _readyHooks;
    private List<Action> _startHooks;
    private List<Action> _stopHooks;
    private bool _applied;
    private bool _readyDone;
    private bool _started;
    private bool _disposed;
    private bool _isTransaction;
    private IServiceProvider? _provider;

    /// <summary>创建根上下文；传入 DI 容器即启用类型化解析兜底（D14）。</summary>
    public ChordContext(IServiceProvider? services = null) {
        _parent = null;
        _children = new List<ChordContext>();
        _uid = NextUid();
        _scope = new Scope(_uid, "root", null);
        _effects = new EffectRegistry();
        _services = new ServiceRegistry();
        _events = new EventEmitter();
        _config = new ConfigStore();
        _waterfalls = new WaterfallRegistry();
        _pending = new List<PendingInjection>();
        _pendingTones = new List<PendingTone>();
        _readyHooks = new List<Action>();
        _startHooks = new List<Action>();
        _stopHooks = new List<Action>();
        _applied = true;
        _readyDone = false;
        _started = false;
        _disposed = false;
        _isTransaction = false;
        _provider = services;
    }

    internal ChordContext(ChordContext parent, string name, object? config) {
        _parent = parent;
        _children = new List<ChordContext>();
        _uid = NextUid();
        _scope = new Scope(_uid, name, config);
        _effects = new EffectRegistry();
        _services = new ServiceRegistry();
        _events = new EventEmitter();
        _config = new ConfigStore();
        _waterfalls = new WaterfallRegistry();
        _pending = new List<PendingInjection>();
        _pendingTones = new List<PendingTone>();
        _readyHooks = new List<Action>();
        _startHooks = new List<Action>();
        _stopHooks = new List<Action>();
        _applied = false;
        _readyDone = false;
        _started = false;
        _disposed = false;
        _isTransaction = false;
        _provider = parent._provider;
        parent._children.Add(this);
    }

    private static int NextUid() {
        int value = _nextUid;
        _nextUid = value + 1;
        return value;
    }

    // ── 结构信息 ──

    /// <summary>父上下文（根为 null）。</summary>
    public ChordContext? Parent { get { return _parent; } }

    /// <summary>作用域（Uid/Name/Status/Config/Error 只读观察）。</summary>
    public IScope Scope { get { return _scope; } }

    /// <summary>全局唯一标识。</summary>
    public int Uid { get { return _uid; } }

    /// <summary>作用域是否处于 Active。</summary>
    public bool IsActive { get { return _scope.Status == ScopeStatus.Active; } }

    /// <summary>是否已释放。</summary>
    public bool IsDisposed { get { return _disposed; } }

    /// <summary>当前账本条目数。</summary>
    public int EffectCount { get { return _effects.Count; } }

    /// <summary>子上下文数。</summary>
    public int ChildCount { get { return _children.Count; } }

    // ── D2 副作用 ──

    /// <summary>
    /// 注册副作用：立即执行 callback 并保存返回的撤销句柄；回调抛异常则
    /// 不保留条目并向调用方传播（D7）。
    /// </summary>
    public IDisposable Effect(Func<IDisposable> callback) {
        EffectEntry entry = _effects.Add(callback);
        return new EffectHandle(entry);
    }

    // ── D5 事件 ──

    /// <summary>订阅事件（撤销 = 退订）。</summary>
    public IDisposable On(string name, Action<object?> listener) {
        return this.On(name, listener, false);
    }

    /// <summary>订阅事件（prepend 插队到队首）。</summary>
    public IDisposable On(string name, Action<object?> listener, bool prepend) {
        EffectEntry entry = _effects.Add(() => _events.Add(name, listener, prepend, false));
        return new EffectHandle(entry);
    }

    /// <summary>订阅事件（触发即退订）。</summary>
    public IDisposable Once(string name, Action<object?> listener) {
        EffectEntry entry = _effects.Add(() => _events.Add(name, listener, false, true));
        return new EffectHandle(entry);
    }

    /// <summary>触发事件：自身 + 后代（DFS，快照遍历）。</summary>
    public void Emit(string name, object? payload) {
        this.EmitSelf(name, payload);
        List<ChordContext> kids = new List<ChordContext>();
        for (int i = 0; i < _children.Count; i++) {
            kids.Add(_children[i]);
        }
        for (int i = 0; i < kids.Count; i++) {
            kids[i].Emit(name, payload);
        }
    }

    /// <summary>触发事件：自身 + 祖先。</summary>
    public void Bubble(string name, object? payload) {
        this.EmitSelf(name, payload);
        ChordContext? up = _parent;
        while (up != null) {
            up!.EmitSelf(name, payload);
            up = up!._parent;
        }
    }

    /// <summary>触发事件：仅自身。</summary>
    public void EmitSelf(string name, object? payload) {
        _events.Emit(name, payload);
    }

    // ── D5.1 瀑布 ──

    /// <summary>
    /// 订阅瀑布：handler(payload, next) 按注册序串联，不调 next 即拦截。
    /// </summary>
    public IDisposable OnWaterfall(string name, Func<object, Func<object, object>, object> handler) {
        return this.OnWaterfall(name, handler, false);
    }

    /// <summary>订阅瀑布（prepend 插队到队首）。</summary>
    public IDisposable OnWaterfall(string name, Func<object, Func<object, object>, object> handler, bool prepend) {
        EffectEntry entry = _effects.Add(() => _waterfalls.Add(name, handler, prepend));
        return new EffectHandle(entry);
    }

    /// <summary>触发瀑布：返回末端产出（无订阅时原样返回）。</summary>
    public object Waterfall(string name, object payload) {
        return _waterfalls.Run(name, payload);
    }

    // ── D3 服务 ──

    /// <summary>
    /// 提供服务（撤销 = 撤销提供，恢复旧条目）；唤醒子树内等待该服务的
    /// 挂起注入与依赖准入音。
    /// </summary>
    public IDisposable Provide(string name, object? instance) {
        EffectEntry entry = _effects.Add(() => {
            IDisposable revert = _services.Provide(name, instance);
            this.WakeSubtree(name);
            return new DisposableAction(() => {
                revert.Dispose();
                this.WakeSubtree(name);
            });
        });
        return new EffectHandle(entry);
    }

    /// <summary>取服务：沿祖先链上溯（本地优先）。</summary>
    public object? GetService(string name) {
        object? local = _services.Get(name);
        if (local != null) {
            return local;
        }
        if (_parent != null) {
            return _parent.GetService(name);
        }
        return null;
    }

    /// <summary>取服务：仅本地。</summary>
    public object? GetLocalService(string name) {
        return _services.Get(name);
    }

    /// <summary>服务是否可达（本地或祖先链）。</summary>
    public bool HasService(string name) {
        if (_services.Has(name)) {
            return true;
        }
        if (_parent != null) {
            return _parent.HasService(name);
        }
        return false;
    }

    // ── D14 类型化服务（类型即契约；DI 容器兜底）──

    /// <summary>类型化提供服务：键 = typeof(T).FullName（撤销 = 撤销提供）。</summary>
    public IDisposable Provide<T>(T instance) where T : class {
        return this.Provide(typeof(T).FullName, instance);
    }

    /// <summary>
    /// 按工厂提供服务：首次解析时构造并缓存（MEDI 工厂语义同构，按需构造）。
    /// </summary>
    public IDisposable Provide<T>(Func<T> factory) where T : class {
        string key = typeof(T).FullName;
        EffectEntry entry = _effects.Add(() => {
            IDisposable revert = _services.ProvideFactory(key, () => (object?)factory());
            this.WakeSubtree(key);
            return new DisposableAction(() => {
                revert.Dispose();
                this.WakeSubtree(key);
            });
        });
        return new EffectHandle(entry);
    }

    /// <summary>类型化取服务：动态阴影链优先，DI 容器兜底。</summary>
    public T? GetService<T>() where T : class {
        object? value = this.ResolveTyped(typeof(T).FullName, typeof(T));
        return value != null ? (T)value : null;
    }

    /// <summary>类型化服务是否可解析（动态在场或 DI 可解析）。</summary>
    public bool HasService<T>() where T : class {
        return this.ResolveTyped(typeof(T).FullName, typeof(T)) != null;
    }

    /// <summary>
    /// 类型化注入：DI 可解析则恒就绪立即执行（静态层不可变）；否则挂起等待
    /// 动态提供，值直入回调。
    /// </summary>
    public IDisposable Inject<T>(Action<ChordContext, T?> callback) where T : class {
        string key = typeof(T).FullName;
        object? di = this.ResolveFromProvider(typeof(T));
        if (di != null) {
            callback(this, (T)di);
            return new DisposableAction(() => { });
        }
        return this.Inject([key], (ChordContext ctx) => {
            object? value = ctx.GetService(key);
            callback(ctx, value != null ? (T)value : null);
        });
    }

    /// <summary>类型化反应式注入：动态依赖消失回滚重跑；DI 依赖恒就绪不受影响。</summary>
    public IDisposable InjectReactive<T>(Action<ChordContext, T?> callback) where T : class {
        string key = typeof(T).FullName;
        object? di = this.ResolveFromProvider(typeof(T));
        if (di != null) {
            callback(this, (T)di);
            return new DisposableAction(() => { });
        }
        return this.InjectReactive([key], (ChordContext ctx) => {
            object? value = ctx.GetService(key);
            callback(ctx, value != null ? (T)value : null);
        });
    }

    private object? ResolveTyped(string key, Type type) {
        object? local = this.GetService(key);
        if (local != null) {
            return local;
        }
        return this.ResolveFromProvider(type);
    }

    private object? ResolveFromProvider(Type type) {
        IServiceProvider? provider = _provider;
        if (provider != null) {
            return provider.GetService(type);
        }
        return null;
    }

    // ── D2 配置 ──

    /// <summary>设置配置（撤销 = 恢复旧值）。</summary>
    public IDisposable SetConfig(string name, object? value) {
        EffectEntry entry = _effects.Add(() => _config.Set(name, value));
        return new EffectHandle(entry);
    }

    /// <summary>取配置：沿祖先链上溯。</summary>
    public object? GetConfig(string name) {
        if (_config.Has(name)) {
            return _config.Get(name);
        }
        if (_parent != null) {
            return _parent.GetConfig(name);
        }
        return null;
    }

    /// <summary>配置是否存在（本地或祖先链）。</summary>
    public bool HasConfig(string name) {
        if (_config.Has(name)) {
            return true;
        }
        if (_parent != null) {
            return _parent.HasConfig(name);
        }
        return false;
    }

    // ── D4 注入 ──

    /// <summary>
    /// 依赖注入：全部依赖就绪 → 立即执行；否则挂起等待。等待期间任一
    /// 「挂起时已在场」的依赖消失 → 注入丢弃（永不执行）。
    /// </summary>
    public IDisposable Inject(string[] names, Action<ChordContext> callback) {
        return this.AddInjection(names, callback, false);
    }

    /// <summary>
    /// 反应式注入：依赖消失 → 自动回滚回调副作用（效果区间撤销）；
    /// 重新可用 → 自动重跑。
    /// </summary>
    public IDisposable InjectReactive(string[] names, Action<ChordContext> callback) {
        return this.AddInjection(names, callback, true);
    }

    private IDisposable AddInjection(string[] names, Action<ChordContext> callback, bool reactive) {
        bool[] wasPresent = new bool[names.Length];
        bool all = true;
        for (int i = 0; i < names.Length; i++) {
            bool has = this.HasService(names[i]);
            wasPresent[i] = has;
            if (!has) {
                all = false;
            }
        }
        PendingInjection p = new PendingInjection(this, names, wasPresent, callback, reactive);
        _pending.Add(p);
        if (all) {
            this.RunInjection(p);
        }
        return new DisposableAction(() => {
            p._dead = true;
        });
    }

    private void RunInjection(PendingInjection p) {
        p._effectStart = _effects.Count;
        p._callback(this);
        p._effectEnd = _effects.Count;
        p._ran = true;
    }

    // ── D1/D7/D12 安装音 ──

    /// <summary>安装音（函数形态，无清理需求）：创建子上下文并立即 Apply。</summary>
    public ChordContext Tone(Action<ChordContext> apply) {
        return this.Tone(apply, null);
    }

    /// <summary>安装音（函数形态 + 配置）。</summary>
    public ChordContext Tone(Action<ChordContext> apply, object? config) {
        return this.ToneImpl(null, apply, null, config, new string[0], "tone");
    }

    /// <summary>安装音（函数形态，返回撤销句柄作为清理动作）。</summary>
    public ChordContext Tone(Func<ChordContext, IDisposable> apply) {
        return this.Tone(apply, null);
    }

    /// <summary>安装音（函数形态，返回撤销句柄 + 配置）。</summary>
    public ChordContext Tone(Func<ChordContext, IDisposable> apply, object? config) {
        return this.ToneImpl(null, null, apply, config, new string[0], "tone");
    }

    /// <summary>安装音（对象形态）。</summary>
    public ChordContext Tone(ITone tone) {
        return this.Tone(tone, null);
    }

    /// <summary>安装音（对象形态 + 配置）；实现 IToneRequirements 时按声明准入（D12）。</summary>
    public ChordContext Tone(ITone tone, object? config) {
        string[] requires = new string[0];
        if (tone is IToneRequirements req) {
            List<string> list = req.Requires;
            requires = new string[list.Count];
            for (int i = 0; i < list.Count; i++) {
                requires[i] = list[i];
            }
        }
        return this.ToneImpl(tone, null, null, config, requires, tone.Name);
    }

    internal ChordContext ToneImpl(ITone? toneObj, Action<ChordContext>? apply, Func<ChordContext, IDisposable>? funcApply,
                              object? config, string[] requires, string name) {
        ChordContext child = new ChordContext(this, name, config);
        if (requires.Length > 0) {
            bool all = true;
            for (int i = 0; i < requires.Length; i++) {
                if (!this.HasService(requires[i])) {
                    all = false;
                }
            }
            if (!all) {
                _pendingTones.Add(new PendingTone(child, requires, toneObj, config, apply));
                return child;
            }
        }
        this.RunApply(toneObj, child, apply, funcApply, config);
        return child;
    }

    private void RunApply(ITone? toneObj, ChordContext child, Action<ChordContext>? apply,
                          Func<ChordContext, IDisposable>? funcApply, object? config) {
        try {
            if (toneObj != null) {
                toneObj.Apply(child, config);
            } else if (funcApply != null) {
                IDisposable disposer = funcApply(child);
                if (disposer != null) {
                    child.Effect(() => {
                        return disposer;
                    });
                }
            } else if (apply != null) {
                apply(child);
            }
            child._scope.SetActive();
            child._applied = true;
            if (this._started) {
                child.Start();
            }
        } catch (Exception e) {
            child._effects.RevertAll();
            child._scope.SetFailed(e.Message);
        }
    }

    // ── D8 热替换 ──

    /// <summary>原位热替换：先装新、成功后再卸旧；失败则旧音保持运行并抛出。</summary>
    public ChordContext Reload(ChordContext oldContext, Action<ChordContext> apply) {
        return this.Reload(oldContext, apply, null);
    }

    /// <summary>原位热替换（含配置）：新音插入旧音原位置（保持音序）。</summary>
    public ChordContext Reload(ChordContext oldContext, Action<ChordContext> apply, object? config) {
        if (oldContext == null || oldContext._parent != this) {
            throw new Exception("Arc.Chord: Reload 目标必须是本上下文的直接子音");
        }
        int index = this.IndexOfChild(oldContext);
        ChordContext fresh = this.Tone(apply, config);
        if (fresh._scope.Status == ScopeStatus.Failed) {
            this._children.Remove(fresh);
            fresh.Stop();
            throw new Exception("Arc.Chord: reload 失败（新音已回滚，旧音保持运行）: " + fresh._scope.Error);
        }
        this._children.Remove(fresh);
        this._children.Insert(index, fresh);
        oldContext.Dispose();
        return fresh;
    }

    private int IndexOfChild(ChordContext child) {
        for (int i = 0; i < _children.Count; i++) {
            if (_children[i] == child) {
                return i;
            }
        }
        return -1;
    }

    // ── D6 事务 ──

    /// <summary>开启副作用事务：事务内一切副作用记入事务账本，Commit 原子合并 / Dispose 回滚。</summary>
    public ChordContext BeginTransaction() {
        ChordContext tx = new ChordContext(this, "transaction", null);
        tx._applied = true;
        tx._isTransaction = true;
        return tx;
    }

    /// <summary>
    /// 提交事务：效果条目、子上下文、挂起注入原子迁移到父上下文；
    /// 已应用子音按父状态即时启动（生命周期钩子即时生效）。
    /// </summary>
    public void Commit() {
        if (!_isTransaction) {
            throw new Exception("Arc.Chord: Commit 仅在事务上下文有效");
        }
        if (_disposed) {
            throw new Exception("Arc.Chord: 事务已释放，无法提交");
        }
        ChordContext parent = this._parent;
        if (parent == null) {
            throw new Exception("Arc.Chord: 事务缺少父上下文");
        }
        this._effects.TransferTo(parent._effects);
        List<ChordContext> kids = new List<ChordContext>();
        for (int i = 0; i < _children.Count; i++) {
            kids.Add(_children[i]);
        }
        for (int i = 0; i < kids.Count; i++) {
            ChordContext kid = kids[i];
            kid._parent = parent;
            parent._children.Add(kid);
            if (parent._started && !kid._started && kid._applied) {
                kid.Start();
            }
        }
        for (int i = 0; i < _pending.Count; i++) {
            PendingInjection p = _pending[i];
            p._owner = parent;
            p._effectStart = 0;
            p._effectEnd = 0;
            parent._pending.Add(p);
        }
        for (int i = 0; i < _pendingTones.Count; i++) {
            parent._pendingTones.Add(_pendingTones[i]);
        }
        _children.Clear();
        _pending.Clear();
        _pendingTones.Clear();
        parent._children.Remove(this);
        _scope.SetDisposed();
        _disposed = true;
    }

    // ── D9 生命周期 ──

    /// <summary>注册 ready 钩子（已过阶段 → 立即执行）。</summary>
    public void OnReady(Action callback) {
        if (_readyDone) {
            callback();
        } else {
            _readyHooks.Add(callback);
        }
    }

    /// <summary>注册 start 钩子（已过阶段 → 立即执行）。</summary>
    public void OnStart(Action callback) {
        if (_started) {
            callback();
        } else {
            _startHooks.Add(callback);
        }
    }

    /// <summary>注册 stop 钩子（Stop/Dispose 时逆序执行）。</summary>
    public void OnStop(Action callback) {
        _stopHooks.Add(callback);
    }

    /// <summary>
    /// 启动：ready 钩子 → start 钩子 → 级联子上下文。未通过依赖准入的
    /// 挂起音（_applied = false）跳过，待依赖就绪后启动。
    /// </summary>
    public void Start() {
        if (_started || _disposed || !_applied) {
            return;
        }
        for (int i = 0; i < _readyHooks.Count; i++) {
            _readyHooks[i]();
        }
        _readyDone = true;
        _started = true;
        for (int i = 0; i < _startHooks.Count; i++) {
            _startHooks[i]();
        }
        List<ChordContext> kids = new List<ChordContext>();
        for (int i = 0; i < _children.Count; i++) {
            kids.Add(_children[i]);
        }
        for (int i = 0; i < kids.Count; i++) {
            kids[i].Start();
        }
    }

    /// <summary>整体卸载：子上下文先于自身释放（逆序）→ stop 钩子（逆序）→ 效果 LIFO 撤销。</summary>
    public void Stop() {
        if (_disposed) {
            return;
        }
        for (int i = _children.Count - 1; i >= 0; i--) {
            _children[i].Stop();
        }
        for (int i = _stopHooks.Count - 1; i >= 0; i--) {
            _stopHooks[i]();
        }
        _effects.RevertAll();
        for (int i = 0; i < _pending.Count; i++) {
            _pending[i]._dead = true;
        }
        for (int i = 0; i < _pendingTones.Count; i++) {
            _pendingTones[i]._dead = true;
        }
        if (_parent != null) {
            _parent.CancelPendingToneFor(this);
        }
        _scope.SetDisposed();
        _disposed = true;
    }

    /// <summary>释放：事务上下文回滚，其余整体卸载。</summary>
    public void Dispose() {
        if (_isTransaction) {
            this.RollbackTransaction();
            return;
        }
        this.Stop();
    }

    private void RollbackTransaction() {
        if (_disposed) {
            return;
        }
        for (int i = _children.Count - 1; i >= 0; i--) {
            _children[i].Dispose();
        }
        _effects.RevertAll();
        for (int i = 0; i < _pending.Count; i++) {
            _pending[i]._dead = true;
        }
        for (int i = 0; i < _pendingTones.Count; i++) {
            _pendingTones[i]._dead = true;
        }
        if (_parent != null) {
            _parent._children.Remove(this);
        }
        _scope.SetDisposed();
        _disposed = true;
    }

    // ── 唤醒（D4/D12）──

    /// <summary>服务变化沿子树传播：重评本上下文挂起项后逐层下探。</summary>
    internal void WakeSubtree(string name) {
        this.CheckPendings(name);
        List<ChordContext> kids = new List<ChordContext>();
        for (int i = 0; i < _children.Count; i++) {
            kids.Add(_children[i]);
        }
        for (int i = 0; i < kids.Count; i++) {
            kids[i].WakeSubtree(name);
        }
    }

    private void CheckPendings(string name) {
        List<PendingInjection> injections = new List<PendingInjection>();
        for (int i = 0; i < _pending.Count; i++) {
            injections.Add(_pending[i]);
        }
        for (int i = 0; i < injections.Count; i++) {
            PendingInjection p = injections[i];
            if (!p._dead && p.ContainsName(name)) {
                this.EvaluateInjection(p);
            }
        }
        List<PendingTone> tones = new List<PendingTone>();
        for (int i = 0; i < _pendingTones.Count; i++) {
            tones.Add(_pendingTones[i]);
        }
        for (int i = 0; i < tones.Count; i++) {
            PendingTone t = tones[i];
            if (!t._dead && t.ContainsName(name) && this.AllServicesPresent(t._names)) {
                t._dead = true;
                this.RunApply(t._toneObj, t._tone, t._apply, null, t._toneConfig);
            }
        }
    }

    private void EvaluateInjection(PendingInjection p) {
        bool all = true;
        bool vanished = false;
        for (int i = 0; i < p._names.Length; i++) {
            bool now = this.HasService(p._names[i]);
            if (now) {
                continue;
            }
            if (p._wasPresent[i]) {
                vanished = true;
            } else {
                all = false;
            }
        }
        if (!p._ran) {
            if (vanished && !p._reactive) {
                p._dead = true;
                return;
            }
            if (all) {
                this.RunInjection(p);
            }
            return;
        }
        if (p._reactive && !all) {
            p.RevertEffects();
            p._ran = false;
            p._effectStart = 0;
            p._effectEnd = 0;
        }
    }

    private bool AllServicesPresent(string[] names) {
        for (int i = 0; i < names.Length; i++) {
            if (!this.HasService(names[i])) {
                return false;
            }
        }
        return true;
    }

    private void CancelPendingToneFor(ChordContext tone) {
        for (int i = 0; i < _pendingTones.Count; i++) {
            if (_pendingTones[i]._tone == tone) {
                _pendingTones[i]._dead = true;
            }
        }
    }
}
