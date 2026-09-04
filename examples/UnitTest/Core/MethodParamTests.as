namespace UnitTest.Core;

using Arc;
using Arc.QIF;

class Printer {
    public string LastPrinted;

    public void Print(int x) {
        LastPrinted = x.ToString();
    }

    public void Print(string s) {
        LastPrinted = s;
    }
}

int Increment(ref int x) {
    x = x + 1;
    return x;
}

bool TryParseInt(string s, out int result) {
    if (s == "42") {
        result = 42;
        return true;
    }
    result = 0;
    return false;
}

int SumDefault(int a, int b = 10, int c = 100) {
    return a + b + c;
}

string Greet(string name, string suffix = "!") {
    return "Hello " + name + suffix;
}

/// <summary>
/// 方法参数单元测试：覆盖 MethodOverload / OptionalParams / RefOutParams 示例。
/// </summary>
public class MethodParamTests
{
    // ── 方法重载 ──

    [Fact]
    public void MethodOverload_Int()
    {
        Printer p = new Printer();
        p.Print(42);
        Assert.True(p.LastPrinted == "42");
    }

    [Fact]
    public void MethodOverload_String()
    {
        Printer p = new Printer();
        p.Print("hello");
        Assert.True(p.LastPrinted == "hello");
    }

    // ── ref 参数 ──

    [Fact]
    public void RefParam_ModifyValue()
    {
        int x = 5;
        int result = Increment(ref x);
        Assert.Equal(6, result);
        Assert.Equal(6, x);
    }

    // ── out 参数 ──

    [Fact]
    public void OutParam_TryParse_Success()
    {
        int result;
        bool ok = TryParseInt("42", out result);
        Assert.True(ok);
        Assert.Equal(42, result);
    }

    [Fact]
    public void OutParam_TryParse_Fail()
    {
        int result;
        bool ok = TryParseInt("abc", out result);
        Assert.False(ok);
        Assert.Equal(0, result);
    }

    // ── 可选参数 ──

    [Fact]
    public void OptionalParam_AllDefaults()
    {
        int result = SumDefault(1);
        Assert.Equal(111, result);
    }

    [Fact]
    public void OptionalParam_OneSupplied()
    {
        int result = SumDefault(1, 20);
        Assert.Equal(121, result);
    }

    [Fact]
    public void OptionalParam_NamedArgument()
    {
        int result = SumDefault(1, c: 200);
        Assert.Equal(211, result);
    }

    [Fact]
    public void OptionalParam_AllNamed()
    {
        int result = SumDefault(a: 5, b: 50, c: 500);
        Assert.Equal(555, result);
    }
}
