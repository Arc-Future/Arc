// 临时诊断——定位 Background 往返仍为 #00000000 的环节，定位后即删。
namespace UnitTest.Arc;

using Arc;
using Arc.QIF;
using Arc.UI;
using Arc.UI.Components;
using Arc.UI.Markup;
using Arc.UI.Media;

public class BrushDiagTests
{
    [Fact]
    public void D1_FromRgba_Components()
    {
        Color c = Color.FromRgba(1.0, 0.0, 1.0, 1.0);
        Assert.Equal(1.0, c.R, 0.001);
        Assert.Equal(0.0, c.G, 0.001);
        Assert.Equal(1.0, c.B, 0.001);
        Assert.Equal(1.0, c.A, 0.001);
    }

    [Fact]
    public void D2_FromRgba_ToHex()
    {
        Color c = Color.FromRgba(1.0, 0.0, 1.0, 1.0);
        Assert.Equal("#FFFF00FF", c.ToHex());
    }

    [Fact]
    public void D3_Parse_ToHex()
    {
        Color c = Color.Parse("#FF0000FF");
        Assert.Equal("#FF0000FF", c.ToHex());
    }

    [Fact]
    public void D4_BrushFromString_ToHex()
    {
        Brush b = Brush.FromString("#FF0000FF");
        Assert.Equal("#FF0000FF", b.ToHex());
    }

    [Fact]
    public void D5_ButtonSetGet_RoundTrip()
    {
        Button b = new Button();
        b.Background = "#FF0000FF";
        Assert.Equal("#FF0000FF", b.Background);
    }

    [Fact]
    public void D6_ButtonSetValue_GetValue()
    {
        Button b = new Button();
        b.SetValue<Brush>(Control.BackgroundProperty, Brush.FromString("#FF0000FF"));
        Brush got = b.GetValue<Brush>(Control.BackgroundProperty);
        Assert.Equal("#FF0000FF", got.ToHex());
    }

    [Fact]
    public void D7_ParseButton_Background()
    {
        ArmlParseResult r = ArmlParser.Parse("<Button Background=\"#FF0000FF\">x</Button>");
        Assert.True(r.Success);
        Button btn = (Button)r.Root;
        Assert.Equal("#FF0000FF", btn.Background);
    }

    [Fact]
    public void D8_SignalBrush_Set()
    {
        Signal<Brush> sig = new Signal<Brush>();
        sig.Set(Brush.FromString("#FF0000FF"));
        Assert.Equal("#FF0000FF", sig.Value.ToHex());
    }

    [Fact]
    public void D9_SignalBrush_CtorInitial()
    {
        Signal<Brush> sig = new Signal<Brush>(Brush.FromString("#FF0000FF"));
        Assert.Equal("#FF0000FF", sig.Value.ToHex());
    }

    [Fact]
    public void D10_Observe_AfterSetValue()
    {
        Button b = new Button();
        b.SetValue<Brush>(Control.BackgroundProperty, Brush.FromString("#FF0000FF"));
        Signal<Brush> sig = b.Observe<Brush>(Control.BackgroundProperty);
        Assert.Equal("#FF0000FF", sig.Value.ToHex());
    }

    [Fact]
    public void D11_SetValue_ThenGetValue_DefaultProp()
    {
        Button b = new Button();
        b.SetValue<Brush>(Control.BackgroundProperty, Brush.FromString("#FF0000FF"));
        Brush got = b.GetValue<Brush>(Control.BackgroundProperty);
        Assert.NotNull(got);
        Assert.Equal("#FF0000FF", got.ToHex());
    }

    // ── P 系列探针：定位 Brush 经泛型边界损坏的确切环节 ──

    [Fact]
    public void P1_PlainFieldAssign_NoGeneric()
    {
        Brush a = Brush.FromString("#FF0000FF");
        Brush b = a;
        Assert.Equal("#FF0000FF", b.ToHex());
    }

    [Fact]
    public void P2_SignalFieldDirect_NoSet()
    {
        Signal<Brush> sig = new Signal<Brush>();
        sig.Value = Brush.FromString("#FF0000FF");
        Assert.Equal("#FF0000FF", sig.Value.ToHex());
    }

    [Fact]
    public void P3_SignalSet_PrebuiltLocal()
    {
        Brush pre = Brush.FromString("#FF0000FF");
        Signal<Brush> sig = new Signal<Brush>();
        sig.Set(pre);
        Assert.Equal("#FF0000FF", sig.Value.ToHex());
    }

    [Fact]
    public void P4_GenericIdentity()
    {
        Brush a = Brush.FromString("#FF0000FF");
        Brush b = BrushDiagTests.Identity<Brush>(a);
        Assert.Equal("#FF0000FF", b.ToHex());
    }

    [Fact]
    public void P5_GenericForward()
    {
        Brush a = Brush.FromString("#FF0000FF");
        Signal<Brush> sig = BrushDiagTests.ForwardIntoSignal<Brush>(a);
        Assert.Equal("#FF0000FF", sig.Value.ToHex());
    }

    [Fact]
    public void P6_BoxCast_Object()
    {
        Brush a = Brush.FromString("#FF0000FF");
        object box = a;
        Brush b = (Brush)box;
        Assert.Equal("#FF0000FF", b.ToHex());
    }

    [Fact]
    public void P7_MinimalGenericSet()
    {
        ProbeSignal<Brush> sig = new ProbeSignal<Brush>();
        Brush pre = Brush.FromString("#FF0000FF");
        sig.Set(pre);
        Assert.Equal("#FF0000FF", sig.Value.ToHex());
    }

    [Fact]
    public void P8_MinimalGenericCtorInitial()
    {
        ProbeSignal<Brush> sig = new ProbeSignal<Brush>(Brush.FromString("#FF0000FF"));
        Assert.Equal("#FF0000FF", sig.Value.ToHex());
    }

    [Fact]
    public void P9_SignalSet_HasSubscriber()
    {
        Signal<Brush> sig = new Signal<Brush>();
        int token = sig.OnChanged((oldV, newV) => { });
        Brush pre = Brush.FromString("#FF0000FF");
        sig.Set(pre);
        sig.Unsubscribe(token);
        Assert.Equal("#FF0000FF", sig.Value.ToHex());
    }

    [Fact]
    public void P10_Replica_Full()
    {
        ProbeSignal2<Brush> sig = new ProbeSignal2<Brush>();
        Brush pre = Brush.FromString("#FF0000FF");
        sig.Set(pre);
        Assert.Equal("#FF0000FF", sig.Value.ToHex());
    }

    [Fact]
    public void P11_Replica_NoNotify()
    {
        ProbeSignal3<Brush> sig = new ProbeSignal3<Brush>();
        Brush pre = Brush.FromString("#FF0000FF");
        sig.Set(pre);
        Assert.Equal("#FF0000FF", sig.Value.ToHex());
    }

    [Fact]
    public void P12_Replica_AssignOnly()
    {
        ProbeSignal4<Brush> sig = new ProbeSignal4<Brush>();
        Brush pre = Brush.FromString("#FF0000FF");
        sig.Set(pre);
        Assert.Equal("#FF0000FF", sig.Value.ToHex());
    }

    [Fact]
    public void P13_TouchAfterStore()
    {
        ProbeSignal5<Brush> sig = new ProbeSignal5<Brush>();
        Brush pre = Brush.FromString("#FF0000FF");
        sig.Set(pre);
        Assert.Equal("#FF0000FF", sig.Value.ToHex());
    }

    [Fact]
    public void P14_TouchBeforeStore()
    {
        ProbeSignal6<Brush> sig = new ProbeSignal6<Brush>();
        Brush pre = Brush.FromString("#FF0000FF");
        sig.Set(pre);
        Assert.Equal("#FF0000FF", sig.Value.ToHex());
    }

    [Fact]
    public void P15_PrivateNoopGenericMethod()
    {
        ProbeSignal7<Brush> sig = new ProbeSignal7<Brush>();
        Brush pre = Brush.FromString("#FF0000FF");
        sig.Set(pre);
        Assert.Equal("#FF0000FF", sig.Value.ToHex());
    }

    [Fact]
    public void P16_TwoTParams()
    {
        ProbeSignal8<Brush> sig = new ProbeSignal8<Brush>();
        Brush pre = Brush.FromString("#FF0000FF");
        sig.Set(pre);
        Assert.Equal("#FF0000FF", sig.Value.ToHex());
    }

    [Fact]
    public void P17_TwoLevel_WithTwoTParams()
    {
        ProbeSignal9<Brush> sig = new ProbeSignal9<Brush>();
        Brush pre = Brush.FromString("#FF0000FF");
        sig.Set(pre);
        Assert.Equal("#FF0000FF", sig.Value.ToHex());
    }

    [Fact]
    public void P18_TwoLevel_AssignOnly()
    {
        ProbeSignal10<Brush> sig = new ProbeSignal10<Brush>();
        Brush pre = Brush.FromString("#FF0000FF");
        sig.Set(pre);
        Assert.Equal("#FF0000FF", sig.Value.ToHex());
    }

    public static T Identity<T>(T x) {
        return x;
    }

    public static Signal<T> ForwardIntoSignal<T>(T x) {
        Signal<T> sig = new Signal<T>();
        sig.Set(x);
        return sig;
    }
}

/// <summary>最小泛型包装——复刻 Signal&lt;T&gt; 的核心结构以区分缺陷层。</summary>
public class ProbeSignal<T> {
    public T Value;

    public ProbeSignal() {
        this.Value = default(T);
    }

    public ProbeSignal(T initial) {
        this.Value = initial;
    }

    public void Set(T newValue) {
        this.Value = newValue;
    }
}

/// <summary>忠实复刻 Signal&lt;T&gt; 的 TrySet 结构（含 old 局部 + 空表 foreach + NotifyChanged）。</summary>
public class ProbeSignal2<T> {
    public T Value;
    private List<Func<T, T, bool>> _changingHandlers;
    private List<Action<T, T>> _changedHandlers;

    public ProbeSignal2() {
        this.Value = default(T);
        _changingHandlers = new List<Func<T, T, bool>>();
        _changedHandlers = new List<Action<T, T>>();
    }

    public void Set(T newValue) {
        this.TrySet(newValue);
    }

    public bool TrySet(T newValue) {
        T old = this.Value;
        if (_changingHandlers != null) {
            foreach (var handler in _changingHandlers) {
                if (handler != null) {
                    if (!handler(old, newValue)) {
                        return false;
                    }
                }
            }
        }
        this.Value = newValue;
        this.NotifyChanged(old, newValue);
        return true;
    }

    private void NotifyChanged(T oldValue, T newValue) {
        if (_changedHandlers == null) { return; }
        foreach (var handler in _changedHandlers) {
            if (handler != null) {
                handler(oldValue, newValue);
            }
        }
    }
}

/// <summary>复刻 TrySet 但去掉 NotifyChanged 调用（保留 old 局部 + 空表 foreach）。</summary>
public class ProbeSignal3<T> {
    public T Value;
    private List<Func<T, T, bool>> _changingHandlers;

    public ProbeSignal3() {
        this.Value = default(T);
        _changingHandlers = new List<Func<T, T, bool>>();
    }

    public void Set(T newValue) {
        T old = this.Value;
        if (_changingHandlers != null) {
            foreach (var handler in _changingHandlers) {
                if (handler != null) {
                    if (!handler(old, newValue)) {
                        return;
                    }
                }
            }
        }
        this.Value = newValue;
    }
}

/// <summary>复刻 TrySet 但去掉 old 局部与 foreach，仅 this.Value = newValue。</summary>
public class ProbeSignal4<T> {
    public T Value;

    public ProbeSignal4() {
        this.Value = default(T);
    }

    public void Set(T newValue) {
        this.Value = newValue;
    }
}

/// <summary>存储后调用私有泛型方法 Touch（不做事）。</summary>
public class ProbeSignal5<T> {
    public T Value;

    public ProbeSignal5() {
        this.Value = default(T);
    }

    public void Set(T newValue) {
        this.Value = newValue;
        this.Touch(newValue);
    }

    private void Touch(T x) {
    }
}

/// <summary>存储前调用私有泛型方法 Touch。</summary>
public class ProbeSignal6<T> {
    public T Value;

    public ProbeSignal6() {
        this.Value = default(T);
    }

    public void Set(T newValue) {
        this.Touch(newValue);
        this.Value = newValue;
    }

    private void Touch(T x) {
    }
}

/// <summary>调用私有泛型方法 Touch 但仅传 0 参数（无 T 实参）。</summary>
public class ProbeSignal7<T> {
    public T Value;

    public ProbeSignal7() {
        this.Value = default(T);
    }

    public void Set(T newValue) {
        this.Value = newValue;
        this.Touch();
    }

    private void Touch() {
    }
}

/// <summary>存储后调用传双 T 参数的私有泛型方法。</summary>
public class ProbeSignal8<T> {
    public T Value;

    public ProbeSignal8() {
        this.Value = default(T);
    }

    public void Set(T newValue) {
        T old = this.Value;
        this.Value = newValue;
        this.Touch2(old, newValue);
    }

    private void Touch2(T a, T b) {
    }
}

/// <summary>两级调用（Set→TrySet），TrySet 内传双 T 参数。</summary>
public class ProbeSignal9<T> {
    public T Value;

    public ProbeSignal9() {
        this.Value = default(T);
    }

    public void Set(T newValue) {
        this.TrySet(newValue);
    }

    public bool TrySet(T newValue) {
        T old = this.Value;
        this.Value = newValue;
        this.Touch2(old, newValue);
        return true;
    }

    private void Touch2(T a, T b) {
    }
}

/// <summary>两级调用（Set→TrySet），TrySet 内仅 this.Value = newValue。</summary>
public class ProbeSignal10<T> {
    public T Value;

    public ProbeSignal10() {
        this.Value = default(T);
    }

    public void Set(T newValue) {
        this.TrySet(newValue);
    }

    public bool TrySet(T newValue) {
        T old = this.Value;
        this.Value = newValue;
        return true;
    }
}
