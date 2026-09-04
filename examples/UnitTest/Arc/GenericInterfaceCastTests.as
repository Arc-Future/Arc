namespace UnitTest.Arc;

using Arc;
using Arc.QIF;

interface ICastable<T> { T Get(); }
class CastImpl : ICastable<int> { public int Get() { return 7; } }

/// <summary>验证 object → 泛型接口运行时 cast + 分派（DI/Mediator 泛型分发的底层面）。</summary>
public class GenericInterfaceCastTests
{
    [Fact]
    public void CastObject_ToGenericInterface_Dispatch()
    {
        object? o = new CastImpl();
        ICastable<int> iface = (ICastable<int>)o;
        Assert.Equal(7, iface.Get());
    }

    [Fact]
    public void Direct_GenericInterface_Dispatch()
    {
        ICastable<int> iface = new CastImpl();
        Assert.Equal(7, iface.Get());
    }
}
