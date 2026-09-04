namespace UnitTest.Core;

using Arc;
using Arc.QIF;

/// <summary>
/// 泛型约束单元测试：覆盖 GenericConstraints 示例的 10 个约束场景。
/// </summary>

// ── 场景 1：泛型类 + 基元类型参数 ──

class SortedList<T> where T : IComparable<T> {
    private T _first;
    private T _second;

    public SortedList(T first, T second) {
        _first = first;
        _second = second;
    }

    public T Min() {
        return _first;
    }

    public T Max() {
        return _second;
    }
}

// ── 场景 2：泛型函数 + 基元类型参数 ──

T MaxOf<T>(T a, T b) where T : IComparable<T> {
    return a;
}

// ── 场景 3：多参数泛型类 + 各自约束 ──

class Pair<T, U> where T : IComparable<T>, U : IComparable<U> {
    public T First;
    public U Second;

    public Pair(T first, U second) {
        First = first;
        Second = second;
    }
}

// ── 场景 4：自定义类显式实现 IComparable<int> ──

class Score : IComparable<int> {
    public int Value;

    public Score(int v) {
        Value = v;
    }

    public int CompareTo(int other) {
        return Value - other;
    }

    public static int Compare(int a, int b) {
        return a - b;
    }
}

// ── 场景 5：泛型类模板显式实现泛型接口 ──

class ComparableBox<T> : IComparable<T> where T : IComparable<T> {
    public T Value;

    public ComparableBox(T v) {
        Value = v;
    }

    public int CompareTo(T other) {
        return 0;
    }

    public static int Compare(T a, T b) {
        return 0;
    }
}

// ── 场景 6：带 where 子句的泛型接口 ──

interface IContainer<T> where T : IComparable<T> {
    T Get();
}

class IntContainer : IContainer<int> {
    private int _v;

    public IntContainer(int v) {
        _v = v;
    }

    public int Get() {
        return _v;
    }
}

// ── 场景 7：class 元约束 ──

class Container<T> where T : class {
    private T _value;

    public Container(T v) {
        _value = v;
    }

    public T Get() {
        return _value;
    }
}

// ── 场景 8：struct 元约束 ──

class Box<T> where T : struct {
    private T _value;

    public Box(T v) {
        _value = v;
    }

    public T Get() {
        return _value;
    }
}

// ── 场景 9+10：new() 构造约束与多约束组合 ──

class Product {
    public int Id;

    public Product() {
        Id = 100;
    }
}

class Factory<T> where T : new() {
    public T Value;

    public Factory(T v) {
        Value = v;
    }
}

class Repository<T> where T : class, new() {
    public T Item;

    public Repository(T item) {
        Item = item;
    }
}

// 未声明任何实例构造函数的类：由隐式 public 无参默认构造满足 new() 约束
class ImplicitDefaultCtor {
    public int Mark;
}

public class GenericConstraintsTests
{
    // ── 场景 1：泛型类 + 基元类型 ──

    [Fact]
    public void ClassConstraint_Int()
    {
        SortedList<int> list = new SortedList<int>(3, 7);
        Assert.Equal(3, list.Min());
        Assert.Equal(7, list.Max());
    }

    // ── 场景 2：泛型函数 + 基元类型 ──

    [Fact]
    public void FunctionConstraint_Int()
    {
        int m = MaxOf<int>(10, 20);
        Assert.Equal(10, m);
    }

    // ── 场景 3：多参数泛型类 + 各自约束 ──

    [Fact]
    public void MultiParamConstraint()
    {
        Pair<int, int> p = new Pair<int, int>(5, 9);
        Assert.Equal(5, p.First);
        Assert.Equal(9, p.Second);
    }

    // ── 场景 4：自定义类显式实现 IComparable<int> ──

    [Fact]
    public void CustomComparableImpl()
    {
        Score s = new Score(42);
        int diff = s.CompareTo(40);
        Assert.Equal(2, diff);
    }

    // ── 场景 5：泛型类模板实例化时检查 IComparable<T> 实现 ──

    [Fact]
    public void GenericClassInterfaceImpl()
    {
        ComparableBox<int> box = new ComparableBox<int>(99);
        int d = box.CompareTo(100);
        Assert.Equal(0, d);
    }

    // ── 场景 6：带 where 子句的泛型接口实例化 ──

    [Fact]
    public void InterfaceWhereClause()
    {
        IntContainer ic = new IntContainer(7);
        Assert.Equal(7, ic.Get());
    }

    // ── 场景 7：class 元约束 ──

    [Fact]
    public void ClassMetaConstraint()
    {
        Container<string> cont = new Container<string>("hello");
        Assert.True(cont.Get() == "hello");
    }

    // ── 场景 8：struct 元约束 ──

    [Fact]
    public void StructMetaConstraint()
    {
        Box<int> sbox = new Box<int>(42);
        Assert.Equal(42, sbox.Get());
    }

    // ── 场景 9：new() 构造约束 ──

    [Fact]
    public void NewConstraint()
    {
        Product prod = new Product();
        Factory<Product> fact = new Factory<Product>(prod);
        Assert.Equal(100, fact.Value.Id);
    }

    // ── 场景 10：多约束组合（class + new()）──

    [Fact]
    public void MultiConstraintWithNew()
    {
        Product prod2 = new Product();
        Repository<Product> repo = new Repository<Product>(prod2);
        Assert.Equal(100, repo.Item.Id);
    }

    // 场景 9b：类未声明任何实例构造函数时满足 new() 约束（隐式默认构造）
    [Fact]
    public void NewConstraint_ImplicitDefaultCtor()
    {
        ImplicitDefaultCtor raw = new ImplicitDefaultCtor();
        raw.Mark = 7;
        Factory<ImplicitDefaultCtor> fact = new Factory<ImplicitDefaultCtor>(raw);
        Assert.Equal(7, fact.Value.Mark);
    }
}
