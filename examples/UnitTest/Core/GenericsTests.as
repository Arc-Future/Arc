namespace UnitTest.Core;

using Arc;
using Arc.QIF;

class GenBox<T> {
    public T Value;

    public GenBox(T value) {
        Value = value;
    }

    public T Get() {
        return Value;
    }
}

T GenIdentity<T>(T x) {
    return x;
}

/// <summary>
/// 泛型与约束单元测试：覆盖 Generics / GenericConstraints 示例。
/// </summary>
public class GenericsTests
{
    // ── 泛型类 ──

    [Fact]
    public void GenericClass_BoxInt()
    {
        GenBox<int> b = new GenBox<int>(42);
        int n = b.Get();
        Assert.Equal(42, n);
    }

    /// <summary>H3 套件：泛型 fluent <c>new T&lt;…&gt;(…).Method()</c> 须完整物化（禁半物化链接裸模板）。</summary>
    [Fact]
    public void GenericClass_FluentNewGet()
    {
        Assert.Equal(42, new GenBox<int>(42).Get());
    }

    [Fact]
    public void GenericClass_FieldAccess()
    {
        GenBox<int> b = new GenBox<int>(100);
        Assert.Equal(100, b.Value);
    }

    // ── 泛型函数 ──

    [Fact]
    public void GenericFunction_IdentityInt()
    {
        int result = GenIdentity<int>(99);
        Assert.Equal(99, result);
    }

    [Fact]
    public void GenericFunction_WithGenericClass()
    {
        GenBox<int> b = new GenBox<int>(42);
        GenBox<int> result = GenIdentity<GenBox<int>>(b);
        Assert.Equal(42, result.Value);
    }

    // ── 嵌套泛型 ctor（L1 门禁：编译成功 ⇒ 语义物化）──

    [Fact]
    public void NestedGenericCtor_InCtorBody()
    {
        NestOuter<int> o = new NestOuter<int>(7);
        Assert.Equal(7, o.Child.V);
    }

    [Fact]
    public void NestedGenericCtor_FromStaticGenericMethod()
    {
        GenBox<int> b = NestHolder.Make<int>(42);
        Assert.Equal(42, b.Get());
    }

    [Fact]
    public void NestedGenericCtor_NestedTypeArg()
    {
        GenBox<GenBox<int>> b = new GenBox<GenBox<int>>(new GenBox<int>(1));
        Assert.Equal(1, b.Value.Value);
    }

    // ── 实例泛型方法更深嵌套（L1 门禁）──

    [Fact]
    public void InstanceGenericMethod_Chain()
    {
        NestChain c = new NestChain();
        GenBox<int> b = c.Wrap<int>(7);
        Assert.Equal(7, b.Get());
    }

    [Fact]
    public void InstanceGenericMethod_OnGenericClass()
    {
        NestMapper<int> m = new NestMapper<int>(1);
        GenBox<string> b = m.Map<string>("hi");
        Assert.Equal("hi", b.Get());
        GenBox<int> c = m.Map<int>(42);
        Assert.Equal(42, c.Get());
    }

    [Fact]
    public void InstanceGenericMethod_NestedCtorViaMethod()
    {
        NestFactory f = new NestFactory();
        NestOuter<int> o = f.Build<int>(7);
        Assert.Equal(7, o.Child.V);
    }
}

class NestInner<T> {
    public T V;
    public NestInner(T v) { V = v; }
}

class NestOuter<T> {
    public NestInner<T> Child;
    public NestOuter(T v) { Child = new NestInner<T>(v); }
}

class NestHolder {
    public static GenBox<T> Make<T>(T v) { return new GenBox<T>(v); }
}

class NestChain {
    public GenBox<T> Leaf<T>(T v) { return new GenBox<T>(v); }
    public GenBox<T> Wrap<T>(T v) { return this.Leaf<T>(v); }
}

class NestMapper<T> {
    public T Seed;
    public NestMapper(T s) { Seed = s; }
    public GenBox<U> Map<U>(U u) { return new GenBox<U>(u); }
}

class NestFactory {
    public NestOuter<T> Build<T>(T v) { return new NestOuter<T>(v); }
}
