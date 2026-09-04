namespace UnitTest.Chord;

using Arc;
using Arc.Chord;
using Arc.Collections;
using Arc.DI;
using Arc.QIF;

/// <summary>
/// Chord 服务/注入/配置面测试（RFC 045 D3/D4/D2 配置）：
/// 阴影注册与后写优先、祖先链可见性、注入就绪/挂起/丢弃/反应式回滚重跑。
/// </summary>
public class ChordServiceTests
{
    [Fact]
    public void Provide_VisibleToDescendantsOnly()
    {
        ChordContext app = new ChordContext();
        ChordContext childA = app.Tone(ctx => { });
        ChordContext childB = app.Tone(ctx => { });
        app.Provide("svc", "value");
        Assert.True(app.HasService("svc"));
        Assert.True(childA.HasService("svc"));
        Assert.Equal("value", (string)childA.GetService("svc"));
        Assert.False(childB.HasService("svc"));
    }

    [Fact]
    public void Provide_RevertRestoresPrevious()
    {
        ChordContext app = new ChordContext();
        IDisposable first = app.Provide("s", "v1");
        IDisposable second = app.Provide("s", "v2");
        Assert.Equal("v2", (string)app.GetService("s"));
        second.Dispose();
        Assert.Equal("v1", (string)app.GetService("s"));
        first.Dispose();
        Assert.False(app.HasService("s"));
    }

    [Fact]
    public void Config_AncestralReadAndRevert()
    {
        ChordContext app = new ChordContext();
        ChordContext child = app.Tone(ctx => { });
        IDisposable handle = app.SetConfig("mode", "fast");
        Assert.Equal("fast", (string)child.GetConfig("mode"));
        Assert.True(child.HasConfig("mode"));
        handle.Dispose();
        Assert.False(app.HasConfig("mode"));
        Assert.False(child.HasConfig("mode"));
    }

    [Fact]
    public void Inject_RunsImmediatelyWhenReady()
    {
        ChordContext app = new ChordContext();
        app.Provide("dep", "d");
        int ran = 0;
        IDisposable handle = app.Inject(["dep"], ctx => { ran = ran + 1; });
        Assert.Equal(1, ran);
        handle.Dispose();
    }

    [Fact]
    public void Inject_PendsUntilProvide()
    {
        ChordContext app = new ChordContext();
        int ran = 0;
        app.Inject(["later"], ctx => { ran = ran + 1; });
        Assert.Equal(0, ran);
        app.Provide("later", "now");
        Assert.Equal(1, ran);
    }

    [Fact]
    public void Inject_DiscardedWhenSatisfiedDependencyVanishes()
    {
        ChordContext app = new ChordContext();
        IDisposable keep = app.Provide("a", "A");
        int ran = 0;
        app.Inject(["a", "b"], ctx => { ran = ran + 1; });
        Assert.Equal(0, ran);
        keep.Dispose();
        app.Provide("b", "B");
        app.Provide("a", "A2");
        Assert.Equal(0, ran);
    }

    [Fact]
    public void InjectReactive_RollsBackAndReruns()
    {
        ChordContext app = new ChordContext();
        IDisposable provider = app.Provide("s", "S");
        int runs = 0;
        app.InjectReactive(["s"], ctx => {
            runs = runs + 1;
            ctx.SetConfig("flag", "on" + runs);
        });
        Assert.Equal(1, runs);
        Assert.Equal("on1", (string)app.GetConfig("flag"));
        provider.Dispose();
        Assert.False(app.HasConfig("flag"));
        Assert.Equal(1, runs);
        app.Provide("s", "S2");
        Assert.Equal(2, runs);
        Assert.Equal("on2", (string)app.GetConfig("flag"));
    }

    [Fact]
    public void Inject_SingleDependencyConvenience()
    {
        ChordContext app = new ChordContext();
        app.Provide("one", "1");
        int ran = 0;
        app.Inject("one", ctx => { ran = ran + 1; });
        Assert.Equal(1, ran);
    }

    
    [Fact]
    public void TypedProvide_ResolveByTypeKey()
    {
        ChordContext app = new ChordContext();
        app.Provide(new Greeter("hi"));
        Assert.True(app.HasService<Greeter>());
        Greeter? g = app.GetService<Greeter>();
        string name = "";
        if (g != null) {
            name = g.Name;
        }
        Assert.Equal("hi", name);
    }

    [Fact]
    public void TypedProvide_ShadowRevertRestoresPrevious()
    {
        ChordContext app = new ChordContext();
        IDisposable first = app.Provide<Greeter>(new Greeter("v1"));
        IDisposable second = app.Provide<Greeter>(new Greeter("v2"));
        Greeter? g2 = app.GetService<Greeter>();
        string name2 = "";
        if (g2 != null) {
            name2 = g2.Name;
        }
        Assert.Equal("v2", name2);
        second.Dispose();
        Greeter? g1 = app.GetService<Greeter>();
        string name1 = "";
        if (g1 != null) {
            name1 = g1.Name;
        }
        Assert.Equal("v1", name1);
        first.Dispose();
        Assert.False(app.HasService<Greeter>());
    }

    [Fact]
    public void FactoryProvide_LazyMaterializeAndCache()
    {
        ChordContext app = new ChordContext();
        int built = 0;
        IDisposable handle = app.Provide<Greeter>(() => {
            built = built + 1;
            return new Greeter("lazy");
        });
        Assert.Equal(0, built);
        Greeter? g = app.GetService<Greeter>();
        Assert.Equal(1, built);
        Greeter? again = app.GetService<Greeter>();
        Assert.Equal(1, built);
        string name = "";
        if (again != null) {
            name = again.Name;
        }
        Assert.Equal("lazy", name);
        handle.Dispose();
        Assert.False(app.HasService<Greeter>());
    }

    [Fact]
    public void TypedInject_ValueFlowsIntoCallback()
    {
        ChordContext app = new ChordContext();
        app.Provide(new Greeter("dep"));
        int ran = 0;
        string name = "";
        app.Inject<Greeter>((ctx, g) => {
            ran = ran + 1;
            if (g != null) {
                name = g.Name;
            }
        });
        Assert.Equal(1, ran);
        Assert.Equal("dep", name);
    }

    [Fact]
    public void TypedInject_PendsUntilProvide()
    {
        ChordContext app = new ChordContext();
        int ran = 0;
        app.Inject<Greeter>((ctx, g) => { ran = ran + 1; });
        Assert.Equal(0, ran);
        app.Provide(new Greeter("late"));
        Assert.Equal(1, ran);
    }

    [Fact]
    public void TypedInject_DIProviderFiresImmediately()
    {
        IServiceCollection services = new ServiceCollection();
        services.AddSingleton<Greeter>();
        IServiceProvider provider = services.Build();
        ChordContext app = new ChordContext(provider);
        int ran = 0;
        app.Inject<Greeter>((ctx, g) => { ran = ran + 1; });
        Assert.Equal(1, ran);
        Assert.True(app.HasService<Greeter>());
        Greeter? g = app.GetService<Greeter>();
        string name = "";
        if (g != null) {
            name = g.Name;
        }
        Assert.Equal("di", name);
    }

    [Fact]
    public void TypedResolve_DynamicShadowsDI()
    {
        IServiceCollection services = new ServiceCollection();
        services.AddSingleton<Greeter>();
        IServiceProvider provider = services.Build();
        ChordContext app = new ChordContext(provider);
        Greeter? fromDi = app.GetService<Greeter>();
        string diName = "";
        if (fromDi != null) {
            diName = fromDi.Name;
        }
        Assert.Equal("di", diName);
        app.Provide<Greeter>(new Greeter("dynamic"));
        Greeter? fromDynamic = app.GetService<Greeter>();
        string dynName = "";
        if (fromDynamic != null) {
            dynName = fromDynamic.Name;
        }
        Assert.Equal("dynamic", dynName);
    }
}
