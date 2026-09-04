namespace UnitTest.Chord;

using Arc;
using Arc.Chord;
using Arc.Collections;
using Arc.QIF;

/// <summary>
/// Chord 生命周期面测试（RFC 045 D1/D6/D7/D8/D9/D11/D12 + 类型化扩展）：
/// 音安装与失败回滚、钩子级联、事务原子性、热替换、依赖准入、贡献点。
/// </summary>
public class ChordLifecycleTests
{
    [Fact]
    public void Tone_ObjectForm_AppliesAndCarriesConfig()
    {
        ChordContext app = new ChordContext();
        ChordContext tone = app.Tone(new TagTone("tag", "cfg-value"));
        Assert.True(tone.IsActive);
        Assert.Equal("tag", tone.Scope.Name);
        Assert.Equal("cfg-value", tone.Scope.Config);
    }

    [Fact]
    public void Tone_ApplyFailureFailsScopeAndRollsBack()
    {
        ChordContext app = new ChordContext();
        List<string> log = new List<string>();
        ChordContext tone = app.Tone(ctx => {
            ctx.Effect(() => {
                log.Add("installed");
                return new DisposableAction(() => log.Add("reverted"));
            });
            throw new Exception("apply boom");
        });
        Assert.False(tone.IsActive);
        Assert.Equal(ScopeStatus.Failed, tone.Scope.Status);
        Assert.Equal("apply boom", tone.Scope.Error);
        Assert.Equal(1, log.Count);
        Assert.Equal("reverted", log[0]);
    }

    [Fact]
    public void Tone_FuncFormCleanupRunsOnTeardown()
    {
        ChordContext app = new ChordContext();
        _cleaned = 0;
        ChordContext tone = app.Tone(ctx => this.MakeCleanup(ctx));
        Assert.Equal(0, _cleaned);
        tone.Dispose();
        Assert.Equal(1, _cleaned);
    }

    private int _cleaned;

    private IDisposable MakeCleanup(ChordContext ctx)
    {
        ctx.On("x", _ => { });
        return new DisposableAction(() => { _cleaned = _cleaned + 1; });
    }

    [Fact]
    public void Start_CascadesAndLateHooksRunImmediately()
    {
        ChordContext app = new ChordContext();
        List<string> log = new List<string>();
        ChordContext tone = app.Tone(ctx => {
            ctx.OnReady(() => { log.Add("ready"); });
            ctx.OnStart(() => { log.Add("start"); });
            ctx.OnStop(() => { log.Add("stop"); });
        });
        Assert.Equal(0, log.Count);
        app.Start();
        Assert.Equal(2, log.Count);
        Assert.Equal("ready", log[0]);
        Assert.Equal("start", log[1]);
        tone.OnReady(() => { log.Add("ready-late"); });
        Assert.Equal(3, log.Count);
        Assert.Equal("ready-late", log[2]);
        app.Stop();
        Assert.Equal(4, log.Count);
        Assert.Equal("stop", log[3]);
    }

    [Fact]
    public void Stop_ChildrenTeardownBeforeParent()
    {
        ChordContext app = new ChordContext();
        List<string> log = new List<string>();
        app.Tone(ctx => {
            ctx.OnStop(() => { log.Add("child-stop"); });
            ctx.Effect(() => {
                return new DisposableAction(() => { log.Add("child-effect"); });
            });
        });
        app.OnStop(() => { log.Add("app-stop"); });
        app.Stop();
        Assert.Equal(3, log.Count);
        Assert.Equal("child-stop", log[0]);
        Assert.Equal("child-effect", log[1]);
        Assert.Equal("app-stop", log[2]);
    }

    [Fact]
    public void Transaction_CommitMergesAtomically()
    {
        ChordContext app = new ChordContext();
        ChordContext tx = app.BeginTransaction();
        tx.Provide("txsvc", "S");
        int hits = 0;
        tx.On("txe", _ => { hits = hits + 1; });
        tx.Commit();
        Assert.True(app.HasService("txsvc"));
        app.Emit("txe", null);
        Assert.Equal(1, hits);
        Assert.True(tx.IsDisposed);
    }

    [Fact]
    public void Transaction_UncommittedDisposeRollsBack()
    {
        ChordContext app = new ChordContext();
        ChordContext tx = app.BeginTransaction();
        tx.Provide("ghost", "G");
        tx.Dispose();
        Assert.False(app.HasService("ghost"));
    }

    [Fact]
    public void Reload_SwapsInPlaceAndDisposesOld()
    {
        ChordContext app = new ChordContext();
        app.Tone(ctx => { });
        ChordContext oldTone = app.Tone(ctx => { ctx.Provide("svc", "v1"); });
        app.Tone(ctx => { });
        ChordContext fresh = app.Reload(oldTone, ctx => { ctx.Provide("svc", "v2"); });
        Assert.True(fresh.IsActive);
        Assert.True(oldTone.IsDisposed);
        Assert.Equal("v2", (string)app.GetService("svc"));
        Assert.Equal(3, app.ChildCount);
    }

    [Fact]
    public void Reload_FailureKeepsOldRunning()
    {
        ChordContext app = new ChordContext();
        ChordContext oldTone = app.Tone(ctx => { ctx.Provide("svc", "v1"); });
        bool threw = false;
        try {
            app.Reload(oldTone, ctx => { throw new Exception("new boom"); });
        } catch (Exception) {
            threw = true;
        }
        Assert.True(threw);
        Assert.False(oldTone.IsDisposed);
        Assert.Equal("v1", (string)app.GetService("svc"));
        Assert.Equal(1, app.ChildCount);
    }

    [Fact]
    public void Requirements_PendingToneStartsWhenProvided()
    {
        ChordContext app = new ChordContext();
        ChordContext tone = app.Tone(new DepTone());
        Assert.False(tone.IsActive);
        Assert.True(tone.Scope.Status == ScopeStatus.Pending);
        Assert.Equal(0, DepTone.AppliedCount);
        app.Provide("dep", "D");
        Assert.True(tone.IsActive);
        Assert.Equal(1, DepTone.AppliedCount);
    }

    [Fact]
    public void Contribute_RegistryRoutesAndAutoReverts()
    {
        ChordContext app = new ChordContext();
        app.Provide<IContributeRegistry>(new ContributeRegistry());
        MenuContributeHost menus = new MenuContributeHost();
        app.AddHost(menus);
        ChordContext tone = app.Tone((ChordContext ctx) => {
            ctx.Contribute("ui.menus", new MenuContribute("build"));
            ctx.Contribute("ui.menus", new MenuContribute("run"));
        });
        Assert.Equal(2, menus.Count);
        tone.Dispose();
        Assert.Equal(0, menus.Count);
    }

    [Fact]
    public void Contribute_MissingRegistryFailsToneNotHost()
    {
        ChordContext app = new ChordContext();
        ChordContext tone = app.Tone((ChordContext ctx) => {
            ctx.Contribute("ui.menus", new MenuContribute("x"));
        });
        Assert.True(tone.Scope.Status == ScopeStatus.Failed);
        Assert.False(app.IsDisposed);
    }

    [Fact]
    public void Contribute_MissingHostFailsToneNotHost()
    {
        ChordContext app = new ChordContext();
        app.Provide<IContributeRegistry>(new ContributeRegistry());
        ChordContext tone = app.Tone((ChordContext ctx) => {
            ctx.Contribute("missing.host", new MenuContribute("x"));
        });
        Assert.True(tone.Scope.Status == ScopeStatus.Failed);
        Assert.False(app.IsDisposed);
    }

    [Fact]
    public void TypedServiceAndEventRoundTrip()
    {
        ChordContext app = new ChordContext();
        app.Provide("g", new Greeter("hi"));
        Greeter? g = app.GetService<Greeter>("g");
        Assert.True(g != null);
        string name = "";
        if (g != null) {
            name = g.Name;
        }
        Assert.Equal("hi", name);
        string heard = "";
        app.On<string>("evt", payload => { heard = payload; });
        app.Emit<string>("evt", "ping");
        Assert.Equal("ping", heard);
    }
}

public class TagTone : ITone {
    private string _name;
    private string _configValue;

    public TagTone(string name, string configValue) {
        _name = name;
        _configValue = configValue;
    }

    public string Name { get { return _name; } }

    public void Apply(ChordContext context, object? config) {
    }
}

public class DepTone : ITone, IToneRequirements {
    public static int AppliedCount = 0;

    public DepTone() {
        AppliedCount = 0;
    }

    public string Name { get { return "dep-tone"; } }

    public List<string> Requires {
        get {
            List<string> requires = new List<string>();
            requires.Add("dep");
            return requires;
        }
    }

    public void Apply(ChordContext context, object? config) {
        AppliedCount = AppliedCount + 1;
        context.SetConfig("dep-applied", "yes");
    }
}

public class MenuContributeHost : IContributeHost {
    private List<IContribute> _entries;

    public MenuContributeHost() {
        _entries = new List<IContribute>();
    }

    public string Id { get { return "ui.menus"; } }

    public int Count { get { return _entries.Count; } }

    public void Register(IContribute contribute, ContributeOptions options) {
        this._entries.Add(contribute);
    }

    public void Unregister(IContribute contribute) {
        this._entries.Remove(contribute);
    }
}

public class MenuContribute : IContribute {
    private string _id;

    public MenuContribute(string id) {
        _id = id;
    }

    public string Id { get { return _id; } }
}

public class Greeter {
    private string _name;

    public Greeter(string name) {
        _name = name;
    }

    public Greeter() {
        _name = "di";
    }

    public string Name { get { return _name; } }
}
