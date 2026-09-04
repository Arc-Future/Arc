namespace UnitTest.Core;

using Arc;
using Arc.QIF;

/// <summary>
/// 自定义数值类型单元测试：覆盖 CustomNumber 示例的 Vector2 : INumber<Vector2>。
/// 验证用户定义类型通过 INumber<T> 参与泛型算法（Sum、AddOne）。
/// </summary>

class Vector2 : INumber<Vector2> {
    public int X;
    public int Y;

    public Vector2(int x, int y) {
        X = x;
        Y = y;
    }

    public static Vector2 Add(Vector2 a, Vector2 b) {
        return new Vector2(a.X + b.X, a.Y + b.Y);
    }

    public static Vector2 Subtract(Vector2 a, Vector2 b) {
        return new Vector2(a.X - b.X, a.Y - b.Y);
    }

    public static Vector2 Multiply(Vector2 a, Vector2 b) {
        return new Vector2(a.X * b.X, a.Y * b.Y);
    }

    public static Vector2 Divide(Vector2 a, Vector2 b) {
        return new Vector2(a.X / b.X, a.Y / b.Y);
    }

    public static Vector2 Negate(Vector2 a) {
        return new Vector2(-a.X, -a.Y);
    }

    public static Vector2 Zero {
        get {
            return new Vector2(0, 0);
        }
    }

    public static Vector2 One {
        get {
            return new Vector2(1, 1);
        }
    }
}

T Sum<T>(T[] items) where T : INumber<T> {
    T sum = T.Zero;
    int n = items.Length;
    int i = 0;
    while (i < n) {
        sum = T.Add(sum, items[i]);
        i = i + 1;
    }
    return sum;
}

T AddOne<T>(T a) where T : INumber<T> {
    return T.Add(a, T.One);
}

public class CustomNumberTests
{
    // ── Vector2 字段初始值 ──

    [Fact]
    public void Vector2_FieldInitialization()
    {
        Vector2 v = new Vector2(7, 13);
        Assert.Equal(7, v.X);
        Assert.Equal(13, v.Y);
    }

    // ── Vector2 加法 ──

    [Fact]
    public void Vector2_Add()
    {
        Vector2 a = new Vector2(1, 2);
        Vector2 b = new Vector2(3, 4);
        Vector2 result = Vector2.Add(a, b);
        Assert.Equal(4, result.X);
        Assert.Equal(6, result.Y);
    }

    // ── Vector2 Zero / One ──

    [Fact]
    public void Vector2_Zero()
    {
        Vector2 z = Vector2.Zero;
        Assert.Equal(0, z.X);
        Assert.Equal(0, z.Y);
    }

    [Fact]
    public void Vector2_One()
    {
        Vector2 o = Vector2.One;
        Assert.Equal(1, o.X);
        Assert.Equal(1, o.Y);
    }

    // ── Sum<int> 复用验证（基元类型走同一泛型算法）──

    [Fact]
    public void Int_Sum_GenericAlgorithm()
    {
        int[] items = [1, 2, 3, 4, 5];
        int result = Sum<int>(items);
        Assert.Equal(15, result);
    }

    // ── Sum<Vector2>：用户类型参与泛型算法 ──

    [Fact]
    public void Vector2_Sum_GenericAlgorithm()
    {
        Vector2[] items = [
            new Vector2(1, 2),
            new Vector2(3, 4),
            new Vector2(5, 6)
        ];
        Vector2 result = Sum<Vector2>(items);
        Assert.Equal(9, result.X);
        Assert.Equal(12, result.Y);
    }

    // ── AddOne<Vector2>：Zero/One 常量属性 + Add ──

    [Fact]
    public void Vector2_AddOne()
    {
        Vector2 v = new Vector2(10, 20);
        Vector2 result = AddOne<Vector2>(v);
        Assert.Equal(11, result.X);
        Assert.Equal(21, result.Y);
    }
}
