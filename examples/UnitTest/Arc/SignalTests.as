namespace UnitTest.Arc;

using Arc;
using Arc.QIF;

/// <summary>
/// 模拟 Element.SetValue&lt;T&gt;：泛型方法体内 new Signal&lt;T&gt;，
/// 经 MIR mono 后必须走真实 __ctor_Signal_*（非 codegen 内联特例）。
/// </summary>
class SignalFactory
{
    public static Signal<T> Make<T>(T initial)
    {
        return new Signal<T>(initial);
    }
}

/// <summary>
/// Signal 构造 / Value / TrySet / Set / OnChanging（泛型委托回调）。
/// </summary>
public class SignalTests
{
    [Fact]
    public void Signal_DefaultValue()
    {
        Signal<int> s = new Signal<int>();
        Assert.Equal(0, s.Value);
    }

    [Fact]
    public void Signal_InitialValue()
    {
        Signal<int> s = new Signal<int>(42);
        Assert.Equal(42, s.Value);
    }

    [Fact]
    public void Signal_FromGenericFactory()
    {
        Signal<int> s = SignalFactory.Make<int>(42);
        Assert.Equal(42, s.Value);
        s.OnChanging((oldV, newV) => newV >= 0);
        Assert.False(s.TrySet(-1));
        Assert.Equal(42, s.Value);
        Assert.True(s.TrySet(7));
        Assert.Equal(7, s.Value);
    }

    [Fact]
    public void TrySet_ChangesValue()
    {
        Signal<int> s = new Signal<int>(0);
        bool ok = s.TrySet(10);
        Assert.True(ok);
        Assert.Equal(10, s.Value);
    }

    [Fact]
    public void Set_ChangesValue()
    {
        Signal<int> s = new Signal<int>(0);
        s.Set(10);
        Assert.Equal(10, s.Value);
    }

    [Fact]
    public void OnChanging_Reject()
    {
        Signal<int> s = new Signal<int>(0);
        s.OnChanging((oldV, newV) => newV >= 0);
        bool ok = s.TrySet(-5);
        Assert.False(ok);
        Assert.Equal(0, s.Value);
    }

    [Fact]
    public void OnChanging_Allow()
    {
        Signal<int> s = new Signal<int>(0);
        s.OnChanging((oldV, newV) => newV >= 0);
        bool ok = s.TrySet(50);
        Assert.True(ok);
        Assert.Equal(50, s.Value);
    }

    [Fact]
    public void OnChanging_BoundsCheck()
    {
        Signal<int> age = new Signal<int>(0);
        age.OnChanging((oldV, newV) => newV >= 0 && newV <= 150);
        Assert.False(age.TrySet(200));
        Assert.Equal(0, age.Value);
        Assert.True(age.TrySet(30));
        Assert.Equal(30, age.Value);
    }
}
