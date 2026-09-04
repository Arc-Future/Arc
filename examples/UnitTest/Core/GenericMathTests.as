namespace UnitTest.Core;

using Arc;
using Arc.QIF;

/// <summary>
/// 泛型数学单元测试：覆盖 GenericMath 示例的 INumber<T> 静态抽象方法。
/// 验证 int / double 两种基元类型的 T.Add/T.Subtract/T.Multiply/T.Divide/
/// T.Negate/T.Zero/T.One/T.Equals/T.Compare/T.GetHashCode。
/// </summary>

T Add<T>(T a, T b) where T : INumber<T> {
    return T.Add(a, b);
}

T Sub<T>(T a, T b) where T : INumber<T> {
    return T.Subtract(a, b);
}

T Mul<T>(T a, T b) where T : INumber<T> {
    return T.Multiply(a, b);
}

T Div<T>(T a, T b) where T : INumber<T> {
    return T.Divide(a, b);
}

T Neg<T>(T a) where T : INumber<T> {
    return T.Negate(a);
}

T SumThree<T>(T a, T b, T c) where T : INumber<T> {
    return T.Add(T.Add(a, b), c);
}

bool Same<T>(T a, T b) where T : INumber<T> {
    return T.Equals(a, b);
}

int Cmp<T>(T a, T b) where T : INumber<T> {
    return T.Compare(a, b);
}

int Hash<T>(T a) where T : INumber<T> {
    return T.GetHashCode(a);
}

T GetZero<T>() where T : INumber<T> {
    return T.Zero;
}

T GetOne<T>() where T : INumber<T> {
    return T.One;
}

public class GenericMathTests
{
    // ── int 加法 ──

    [Fact]
    public void Int_Add()
    {
        int result = Add<int>(3, 4);
        Assert.Equal(7, result);
    }

    // ── double 加法 ──

    [Fact]
    public void Double_Add()
    {
        double result = Add<double>(1.5, 2.5);
        Assert.True(result == 4.0);
    }

    // ── int 乘法 ──

    [Fact]
    public void Int_Multiply()
    {
        int result = Mul<int>(6, 7);
        Assert.Equal(42, result);
    }

    // ── double 乘法 ──

    [Fact]
    public void Double_Multiply()
    {
        double result = Mul<double>(2.0, 3.5);
        Assert.True(result == 7.0);
    }

    // ── int 减法 ──

    [Fact]
    public void Int_Subtract()
    {
        int result = Sub<int>(10, 3);
        Assert.Equal(7, result);
    }

    // ── double 减法 ──

    [Fact]
    public void Double_Subtract()
    {
        double result = Sub<double>(5.0, 1.5);
        Assert.True(result == 3.5);
    }

    // ── int 除法 ──

    [Fact]
    public void Int_Divide()
    {
        int result = Div<int>(20, 4);
        Assert.Equal(5, result);
    }

    // ── double 除法 ──

    [Fact]
    public void Double_Divide()
    {
        double result = Div<double>(7.0, 2.0);
        Assert.True(result == 3.5);
    }

    // ── int 取负 ──

    [Fact]
    public void Int_Negate()
    {
        int result = Neg<int>(5);
        Assert.Equal(-5, result);
    }

    // ── double 取负 ──

    [Fact]
    public void Double_Negate()
    {
        double result = Neg<double>(2.5);
        Assert.True(result == -2.5);
    }

    // ── int Zero / One ──

    [Fact]
    public void Int_ZeroOne()
    {
        Assert.Equal(0, GetZero<int>());
        Assert.Equal(1, GetOne<int>());
    }

    // ── double Zero / One ──

    [Fact]
    public void Double_ZeroOne()
    {
        Assert.True(GetZero<double>() == 0.0);
        Assert.True(GetOne<double>() == 1.0);
    }

    // ── int SumThree ──

    [Fact]
    public void Int_SumThree()
    {
        int result = SumThree<int>(1, 2, 3);
        Assert.Equal(6, result);
    }

    // ── Equals ──

    [Fact]
    public void Int_Equals()
    {
        Assert.True(Same<int>(5, 5));
        Assert.False(Same<int>(5, 6));
    }

    // ── Compare ──

    [Fact]
    public void Int_Compare()
    {
        Assert.Less(Cmp<int>(3, 5), 0);
        Assert.Greater(Cmp<int>(5, 3), 0);
        Assert.Equal(0, Cmp<int>(5, 5));
    }

    // ── GetHashCode ──

    [Fact]
    public void Int_GetHashCode()
    {
        Assert.Equal(42, Hash<int>(42));
    }
}
