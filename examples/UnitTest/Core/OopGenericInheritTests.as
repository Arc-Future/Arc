namespace UnitTest.Core;

using Arc;
using Arc.QIF;

class OopGenBase {
    // void return + 泛型参数（继承侧调用）
    public void SetVal<T>(T val) { }
    public void SetBox<T>(OopGenBox<T> box, T v) { box.Value = v; }
}

class OopGenDerived : OopGenBase {
    public void Test() {
        this.SetVal<int>(42);
    }
    public void TestBox(OopGenBox<int> box, int v) {
        this.SetBox<int>(box, v);
    }
}

class OopGenBox<T> {
    public T Value;
    public OopGenBox(T v) { Value = v; }
}

/// <summary>
/// 继承链上调用基类泛型实例方法（原 Deferred/OopBug + examples/OopFix）。
/// </summary>
public class OopGenericInheritTests
{
    [Fact]
    public void VoidGeneric_NoComplexParam()
    {
        OopGenDerived d = new OopGenDerived();
        d.Test();
    }

    [Fact]
    public void VoidGeneric_BoxParam_Mutates()
    {
        OopGenDerived d = new OopGenDerived();
        OopGenBox<int> b = new OopGenBox<int>(10);
        d.TestBox(b, 99);
        Assert.Equal(99, b.Value);
    }
}
