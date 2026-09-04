namespace UnitTest.Chord;

using Arc;
using Arc.Chord;
using Arc.Collections;
using Arc.QIF;

/// <summary>
/// Chord 副作用与事件面测试（RFC 045 D2/D5/D5.1）：
/// Effect LIFO 撤销与幂等、事件三级广播、Once/prepend、瀑布管道。
/// </summary>
public class ChordEffectTests
{
    [Fact]
    public void Effect_RevertsInLifoOnDispose()
    {
        ChordContext app = new ChordContext();
        List<string> log = new List<string>();
        app.Effect(() => {
            log.Add("a");
            return new DisposableAction(() => log.Add("a-"));
        });
        app.Effect(() => {
            log.Add("b");
            return new DisposableAction(() => log.Add("b-"));
        });
        app.Dispose();
        Assert.Equal(4, log.Count);
        Assert.Equal("b-", log[2]);
        Assert.Equal("a-", log[3]);
    }

    [Fact]
    public void Effect_HandleDisposeIsIdempotent()
    {
        ChordContext app = new ChordContext();
        int count = 0;
        IDisposable handle = app.Effect(() => {
            return new DisposableAction(() => { count = count + 1; });
        });
        handle.Dispose();
        handle.Dispose();
        handle.Dispose();
        Assert.Equal(1, count);
    }

    [Fact]
    public void Effect_CallbackThrowDoesNotKeepEntry()
    {
        ChordContext app = new ChordContext();
        bool threw = false;
        try {
            app.Effect(this.MakeBoom);
        } catch (Exception) {
            threw = true;
        }
        Assert.True(threw);
        Assert.Equal(0, app.EffectCount);
    }

    private IDisposable MakeBoom()
    {
        throw new Exception("boom");
    }

    [Fact]
    public void Emit_ReachesDescendantsNotSiblings()
    {
        ChordContext app = new ChordContext();
        ChordContext childA = app.Tone(ctx => { });
        ChordContext childB = app.Tone(ctx => { });
        int appHits = 0;
        int aHits = 0;
        int bHits = 0;
        app.On("ping", _ => { appHits = appHits + 1; });
        childA.On("ping", _ => { aHits = aHits + 1; });
        childB.On("ping", _ => { bHits = bHits + 1; });
        app.Emit("ping", null);
        Assert.Equal(1, appHits);
        Assert.Equal(1, aHits);
        Assert.Equal(1, bHits);
        childB.Emit("ping", null);
        Assert.Equal(1, appHits);
        Assert.Equal(1, aHits);
        Assert.Equal(2, bHits);
    }

    [Fact]
    public void Bubble_ReachesAncestors()
    {
        ChordContext app = new ChordContext();
        ChordContext child = app.Tone(ctx => { });
        int appHits = 0;
        int childHits = 0;
        app.On("up", _ => { appHits = appHits + 1; });
        child.On("up", _ => { childHits = childHits + 1; });
        child.Bubble("up", null);
        Assert.Equal(1, appHits);
        Assert.Equal(1, childHits);
    }

    [Fact]
    public void Once_FiresExactlyOnce()
    {
        ChordContext app = new ChordContext();
        int hits = 0;
        app.Once("e", _ => { hits = hits + 1; });
        app.EmitSelf("e", null);
        app.EmitSelf("e", null);
        Assert.Equal(1, hits);
    }

    [Fact]
    public void On_PrependRunsFirst()
    {
        ChordContext app = new ChordContext();
        List<string> order = new List<string>();
        app.On("k", _ => { order.Add("first"); });
        app.On("k", _ => { order.Add("prepended"); }, true);
        app.EmitSelf("k", null);
        Assert.Equal(2, order.Count);
        Assert.Equal("prepended", order[0]);
        Assert.Equal("first", order[1]);
    }

    [Fact]
    public void Waterfall_ChainsInRegistrationOrder()
    {
        ChordContext app = new ChordContext();
        app.OnWaterfall("w", (payload, next) => {
            return next("[" + payload + "]");
        });
        app.OnWaterfall("w", (payload, next) => {
            return next(payload + "!");
        });
        object? result = app.Waterfall("w", "x");
        Assert.Equal("[x]!", (string)result);
    }

    [Fact]
    public void Waterfall_HandlerWithoutNextIntercepts()
    {
        ChordContext app = new ChordContext();
        app.OnWaterfall("w", (payload, next) => {
            return next(payload);
        });
        app.OnWaterfall("w", (payload, next) => {
            return "intercepted";
        });
        object? result = app.Waterfall("w", "seed");
        Assert.Equal("intercepted", (string)result);
    }

    [Fact]
    public void Waterfall_NoListenerReturnsPayloadUnchanged()
    {
        ChordContext app = new ChordContext();
        object? result = app.Waterfall("missing", "passthrough");
        Assert.Equal("passthrough", (string)result);
    }

    [Fact]
    public void Waterfall_PrependJumpsQueue()
    {
        ChordContext app = new ChordContext();
        app.OnWaterfall("p", (payload, next) => {
            return next("tail+" + payload);
        });
        app.OnWaterfall("p", (payload, next) => {
            return next("head>" + payload);
        }, true);
        object? result = app.Waterfall("p", "0");
        Assert.Equal("tail+head>0", (string)result);
    }

    [Fact]
    public void Waterfall_UnsubscribeRemovesHandler()
    {
        ChordContext app = new ChordContext();
        IDisposable handle = app.OnWaterfall("u", (payload, next) => {
            return "gone";
        });
        handle.Dispose();
        object? result = app.Waterfall("u", "kept");
        Assert.Equal("kept", (string)result);
    }
}
