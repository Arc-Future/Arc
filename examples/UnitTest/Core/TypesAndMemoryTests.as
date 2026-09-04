namespace UnitTest.Core;

using Arc;
using Arc.QIF;

// ── Properties ──

class PropCounter {
    public int Count { get; set; }

    public int Doubled {
        get { return Count * 2; }
    }
}

/// 表达式体属性 / 访问器 / 方法（C# `=>` 脱糖为既有 get/set/方法块）。
class ExprBodiedBox {
    private int _value;

    public ExprBodiedBox(int value) {
        _value = value;
    }

    public int Value => _value;

    public int Wrapped {
        get => _value;
        set => _value = value;
    }

    public int Doubled() => Value * 2;
}

// ── Const/Readonly ──

class Calculator {
    public const int Multiplier = 2;
    public const string Label = "Calc";
    public const bool Enabled = true;
    public readonly int Offset;

    public Calculator(int offset) {
        Offset = offset;
    }
}

// ── Virtual Members ──

class Shape {
    public virtual string Kind() { return "shape"; }
    public virtual string Name { get { return "shape"; } }
}

class Circle : Shape {
    public override string Kind() { return "circle"; }
    public override string Name { get { return "circle"; } }
}

class Square : Shape {
    public override string Kind() { return "square"; }
    public override string Name { get { return "square"; } }
}

// ── Nullable helpers ──

class NullableContainer {
    public string Label;
    public string? MaybeNull;

    public NullableContainer(string label) {
        Label = label;
        MaybeNull = null;
    }
}

/// <summary>
/// 类型与内存模型单元测试：覆盖 Properties / ConstReadonly / VirtualMembers / 
/// Covariance / MemoryModel / NullableTypes / DefaultExpr / VarInference 示例。
/// </summary>
public class TypesAndMemoryTests
{
    // ── Properties ──

    [Fact]
    public void Property_GetSet()
    {
        PropCounter c = new PropCounter();
        c.Count = 42;
        Assert.Equal(42, c.Count);
    }

    [Fact]
    public void Property_CompoundAssign()
    {
        PropCounter c = new PropCounter();
        c.Count = 10;
        c.Count += 5;
        Assert.Equal(15, c.Count);
    }

    [Fact]
    public void Property_GetOnly()
    {
        PropCounter c = new PropCounter();
        c.Count = 7;
        Assert.Equal(14, c.Doubled);
    }

    [Fact]
    public void Property_ExpressionBodied()
    {
        ExprBodiedBox box = new ExprBodiedBox(21);
        Assert.Equal(21, box.Value);
        box.Wrapped = 11;
        Assert.Equal(11, box.Value);
        Assert.Equal(11, box.Wrapped);
        Assert.Equal(22, box.Doubled());
    }

    // ── Const / Readonly ──

    [Fact]
    public void Const_Int()
    {
        Calculator calc = new Calculator(10);
        int value = Calculator.Multiplier;
        Assert.Equal(2, value);
    }

    [Fact]
    public void Const_String()
    {
        Assert.True(Calculator.Label == "Calc");
    }

    [Fact]
    public void Const_Bool()
    {
        Assert.True(Calculator.Enabled);
    }

    [Fact]
    public void Readonly_Field()
    {
        Calculator calc = new Calculator(5);
        Assert.Equal(5, calc.Offset);
    }

    // ── Virtual Members ──

    [Fact]
    public void Virtual_Method_Dispatch()
    {
        Shape s = new Circle();
        Assert.True(s.Kind() == "circle");
    }

    [Fact]
    public void Virtual_Property_Dispatch()
    {
        Shape s = new Square();
        Assert.True(s.Name == "square");
    }

    [Fact]
    public void Virtual_Method_BaseType()
    {
        Shape s = new Shape();
        Assert.True(s.Kind() == "shape");
    }

    [Fact]
    public void Virtual_CrossDispatch()
    {
        Shape c = new Circle();
        Shape sq = new Square();
        Assert.True(c.Kind() == "circle");
        Assert.True(sq.Kind() == "square");
    }

    // ── 内存模型：值类型 vs 引用类型 ──

    [Fact]
    public void Class_IsReferenceType()
    {
        PropCounter a = new PropCounter();
        a.Count = 1;
        PropCounter b = a;
        b.Count = 99;
        Assert.Equal(99, a.Count);
    }

    // ── 值类型 (struct) ──

    [Fact]
    public void Struct_IsValueType()
    {
        int a = 1;
        int b = a;
        b = 99;
        Assert.Equal(1, a);
    }

    // ── default(T) ──

    [Fact]
    public void Default_Int()
    {
        int n = default(int);
        Assert.Equal(0, n);
    }

    [Fact]
    public void Default_Bool()
    {
        bool b = default(bool);
        Assert.False(b);
    }

    // ── 可空引用类型 ──

    [Fact]
    public void Nullable_NullCoalescing()
    {
        string? s = null;
        string result = s ?? "default";
        Assert.True(result == "default");
    }

    [Fact]
    public void Nullable_NullCoalescing_NotNull()
    {
        string? s = "hello";
        string result = s ?? "default";
        Assert.True(result == "hello");
    }

    [Fact]
    public void Nullable_NullCheck_Narrowing()
    {
        string? s = "hello";
        if (s != null) {
            Assert.Equal(5, s.Length);
        } else {
            Assert.True(false);
        }
    }

    // ── var 推断 ──

    [Fact]
    public void Var_Inference_Int()
    {
        var x = 42;
        Assert.Equal(42, x);
    }

    [Fact]
    public void Var_Inference_String()
    {
        var s = "hello";
        Assert.True(s == "hello");
    }

    [Fact]
    public void Var_Inference_Bool()
    {
        var b = true;
        Assert.True(b);
    }
}
