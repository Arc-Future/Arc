namespace UnitTest.Core;

using Arc;
using Arc.QIF;

/// <summary>
/// 嵌套 new 表达式单元测试：覆盖 NestedNew 示例的两种路径。
/// 1. new X(new Y()) — 构造函数参数路径
/// 2. obj.Foo(new X()) — 方法调用参数路径
/// </summary>

class Inner {
    public int Value;
    public Inner() {
        Value = 42;
    }
}

class Outer {
    public Inner Inner;
    public Outer(Inner i) {
        Inner = i;
    }
}

class NestedContainer {
    public NestedItem Item;
    public NestedContainer() {
        Item = new NestedItem(1);
    }
    public void SetItem(NestedItem it) {
        Item = it;
    }
}

class NestedItem {
    public int Id;
    public NestedItem(int id) {
        Id = id;
    }
}

public class NestedNewTests
{
    // ── 构造函数参数路径：new X(new Y()) ──

    [Fact]
    public void ConstructorArg_NestedNew()
    {
        Outer o = new Outer(new Inner());
        Assert.Equal(42, o.Inner.Value);
    }

    // ── 方法调用参数路径：obj.Foo(new X()) ──

    [Fact]
    public void MethodCallArg_NestedNew()
    {
        NestedContainer c = new NestedContainer();
        c.SetItem(new NestedItem(99));
        Assert.Equal(99, c.Item.Id);
    }
}
