namespace UnitTest.Core;

using Arc;
using Arc.QIF;

/// <summary>
/// 委托与 Lambda 单元测试：覆盖 Delegates / LambdaCapture 示例。
/// </summary>
public class DelegatesTests
{
    // ── 基本委托 ──

    [Fact]
    public void Func_Lambda_AddOne()
    {
        Func<int, int> f = x => x + 1;
        int result = f(5);
        Assert.Equal(6, result);
    }

    [Fact]
    public void Func_Lambda_Multiply()
    {
        Func<int, int> f = x => x * 3;
        int result = f(7);
        Assert.Equal(21, result);
    }

    // ── Lambda 值捕获 ──

    [Fact]
    public void Lambda_ValueCapture()
    {
        int offset = 10;
        Func<int, int> f = x => x + offset;
        int result = f(5);
        Assert.Equal(15, result);
    }

    // ── 多参数 Lambda ──

    [Fact]
    public void Lambda_MultiArg()
    {
        Func<int, int> f = x => x * 2;
        int a = f(3);
        int b = f(5);
        Assert.Equal(6, a);
        Assert.Equal(10, b);
    }

    // ── 实例字段存委托后再调用（禁 @_f 直调半物化）──

    [Fact]
    public void Func_Field_NoCapture_Invoke()
    {
        FuncFieldHolder h = new FuncFieldHolder();
        h.Set(x => x + 1);
        Assert.Equal(6, h.Run(5));
    }
}

public class FuncFieldHolder
{
    private Func<int, int> _f;

    public void Set(Func<int, int> f)
    {
        _f = f;
    }

    public int Run(int x)
    {
        return _f(x);
    }
}
