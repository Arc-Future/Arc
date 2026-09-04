// RFC 007 M2a：插值字符串深水面——alignment / format / 组合 / 表达式洞。
// 覆盖：{x,5}（PadLeft 右对齐）、{x,-5}（PadRight 左对齐）、
//       D5（零填充十进制）、X/x（十六进制）、F（定点）、N（千分位）、
//       P（百分比）、C（货币）、E（科学计数）、G（最短），
//       {expr,align:format} 组合、表达式洞（二元运算 / 方法调用）、多占位混合。
// 诚实面（以 builtin_primitive.rs / rt_parse.c / check_expr.rs 实测为准）：
//   - bool/char 拒绝格式说明符（typeck 硬错误），无对应用例；
//   - 格式仅限整数族 + float/double（其余类型 typeck 拒绝）。

namespace UnitTest.Arc;

using Arc;
using Arc.QIF;

/// <summary>
/// 插值字符串 alignment / format 深水面单元测试（RFC 007 M2a+）。
/// </summary>
public class StringInterpolationTests
{
    // ── 对齐：{x,align} ──

    [Fact]
    public void Interp_Align_Right_PadLeft()
    {
        int x = 42;
        string s = $"[{x,5}]";
        Assert.True(s == "[   42]");
        Assert.Equal(7, s.Length);
    }

    [Fact]
    public void Interp_Align_Left_PadRight()
    {
        int x = 42;
        string s = $"[{x,-5}]";
        Assert.True(s == "[42   ]");
        Assert.Equal(7, s.Length);
    }

    [Fact]
    public void Interp_Align_Narrower_NoChange()
    {
        string word = "hello";
        string s = $"[{word,3}]";
        Assert.True(s == "[hello]");
        string left = $"[{word,-3}]";
        Assert.True(left == "[hello]");
    }

    [Fact]
    public void Interp_Align_StringVar()
    {
        string name = "arc";
        string s = $"[{name,6}]";
        Assert.True(s == "[   arc]");
    }

    // ── 格式：D 十进制（零填充）──

    [Fact]
    public void Interp_Format_D5_Int()
    {
        int n = 42;
        string s = $"[{n:D5}]";
        Assert.True(s == "[00042]");
    }

    [Fact]
    public void Interp_Format_D5_Negative()
    {
        int n = -42;
        // C#：精度只计数字位，负号在零填充数字之前（-42 → D5 → "-00042"）。
        string s = $"[{n:D5}]";
        Assert.True(s == "[-00042]");
    }

    [Fact]
    public void Interp_Format_D_NoPrecision_Int()
    {
        int n = 42;
        string s = $"[{n:D}]";
        Assert.True(s == "[42]");
    }

    [Fact]
    public void Interp_Format_D5_Long()
    {
        long n = 1234567;
        string s = $"[{n:D9}]";
        Assert.True(s == "[001234567]");
    }

    // ── 格式：X / x 十六进制 ──

    [Fact]
    public void Interp_Format_X_Int()
    {
        int n = 255;
        string s = $"[{n:X}]";
        Assert.True(s == "[FF]");
    }

    [Fact]
    public void Interp_Format_x_Int()
    {
        int n = 255;
        string s = $"[{n:x}]";
        Assert.True(s == "[ff]");
    }

    [Fact]
    public void Interp_Format_X8_Precision()
    {
        int n = 255;
        string s = $"[{n:X8}]";
        Assert.True(s == "[000000FF]");
    }

    [Fact]
    public void Interp_Format_X_Long()
    {
        long n = 48879;
        string s = $"[{n:X}]";
        Assert.True(s == "[BEEF]");
    }

    // ── 格式：F 定点 ──

    [Fact]
    public void Interp_Format_F2_Double()
    {
        double pi = 3.14159;
        string s = $"pi={pi:F2}";
        Assert.True(s == "pi=3.14");
    }

    [Fact]
    public void Interp_Format_F0_Double()
    {
        double v = 2.7;
        string s = $"[{v:F0}]";
        Assert.True(s == "[3]");
    }

    [Fact]
    public void Interp_Format_F1_Float()
    {
        float v = 2.5;
        string s = $"[{v:F1}]";
        Assert.True(s == "[2.5]");
    }

    // ── 格式：N 千分位 / P 百分比 / C 货币 / E 科学 / G 最短 ──

    [Fact]
    public void Interp_Format_N2_Int()
    {
        int n = 1234567;
        string s = $"[{n:N2}]";
        Assert.True(s == "[1,234,567.00]");
    }

    [Fact]
    public void Interp_Format_P_Double()
    {
        double ratio = 0.25;
        // InvariantCulture P：乘 100 + 空格 + 百分号。
        string s = $"[{ratio:P}]";
        Assert.True(s == "[25.00 %]");
    }

    [Fact]
    public void Interp_Format_C_Double()
    {
        double price = 1234.5;
        // InvariantCulture C：¤（U+00A4）前缀。
        string s = $"[{price:C}]";
        Assert.True(s == "[¤1,234.50]");
    }

    [Fact]
    public void Interp_Format_E2_Double()
    {
        double v = 1234.5;
        // C#：指数至少 3 位。
        string s = $"[{v:E2}]";
        Assert.True(s == "[1.23E+003]");
    }

    [Fact]
    public void Interp_Format_G_Double()
    {
        double v = 1234.5;
        string s = $"[{v:G}]";
        Assert.True(s == "[1234.5]");
    }

    // ── 对齐 + 格式组合：{expr,align:format} ──

    [Fact]
    public void Interp_AlignFormat_Combined_Right()
    {
        double pi = 3.14159;
        string s = $"[{pi,8:F2}]";
        Assert.True(s == "[    3.14]");
        Assert.Equal(10, s.Length);
    }

    [Fact]
    public void Interp_AlignFormat_Combined_Left()
    {
        int n = 42;
        string s = $"[{n,-7:D5}]";
        Assert.True(s == "[00042  ]");
    }

    [Fact]
    public void Interp_AlignFormat_X_Right()
    {
        int n = 255;
        string s = $"[{n,6:X2}]";
        Assert.True(s == "[    FF]");
    }

    // ── 表达式洞：二元运算 / 方法调用 ──

    [Fact]
    public void Interp_Expr_Binary()
    {
        int a = 10;
        int b = 20;
        string s = $"sum={a + b}";
        Assert.True(s == "sum=30");
    }

    [Fact]
    public void Interp_Expr_Binary_WithFormat()
    {
        int a = 7;
        int b = 5;
        string s = $"[{a * b,4:D3}]";
        // 35 → D3 → "035"（宽 3）→ PadLeft(4) 补 1 空格。
        Assert.True(s == "[ 035]");
    }

    [Fact]
    public void Interp_Expr_MethodCall()
    {
        string name = "arc";
        string s = $"lang={name.ToUpper()}";
        Assert.True(s == "lang=ARC");
    }

    [Fact]
    public void Interp_Expr_MethodCall_WithAlign()
    {
        string name = "arc";
        string s = $"[{name.ToUpper(),5}]";
        Assert.True(s == "[  ARC]");
    }

    // ── 多占位混合：字符串 + 整数 + 浮点 ──

    [Fact]
    public void Interp_Multi_MixedTypes()
    {
        string name = "arc";
        int major = 1;
        double ratio = 0.5;
        string s = $"{name} v{major} ({ratio:P0})";
        Assert.True(s == "arc v1 (50 %)");
    }

    [Fact]
    public void Interp_Multi_AlignAndFormat()
    {
        string lhs = "id";
        int id = 7;
        double score = 91.25;
        string s = $"{lhs}={id,3:D3} score={score,6:F1}";
        Assert.True(s == "id=007 score=  91.2");
    }

    [Fact]
    public void Interp_Multi_BackToBackHoles()
    {
        int a = 1;
        int b = 2;
        string s = $"{a}{b}";
        Assert.True(s == "12");
    }

    // ── 边界：宽度不足不截断、负数对齐 ──

    [Fact]
    public void Interp_Align_Negative_Number()
    {
        int n = -42;
        string s = $"[{n,6}]";
        Assert.True(s == "[   -42]");
        string left = $"[{n,-6}]";
        Assert.True(left == "[-42   ]");
    }

    [Fact]
    public void Interp_Format_F2_Negative()
    {
        double v = -3.14159;
        string s = $"[{v:F2}]";
        Assert.True(s == "[-3.14]");
    }
}
