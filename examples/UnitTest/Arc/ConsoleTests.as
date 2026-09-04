namespace UnitTest.Arc;

using Arc;
using Arc.QIF;

/// <summary>
/// Console API 单元测试。
/// Console 方法签名已对齐 runtime ABI（int 参数和返回值）。
/// 注意：runtime Get* 函数返回默认值（Gray=7/Black=0），不跟踪 Set* 状态
/// （避免解析终端响应导致阻塞），因此 Set 后立即 Get 不会得到 Set 的值。
/// </summary>
public class ConsoleTests
{
    [Fact]
    public void GetForegroundColor_Default()
    {
        int fg = Console.GetForegroundColor();
        Assert.Equal(7, fg);
    }

    [Fact]
    public void GetBackgroundColor_Default()
    {
        int bg = Console.GetBackgroundColor();
        Assert.Equal(0, bg);
    }

    [Fact]
    public void SetForegroundColor_DoesNotThrow()
    {
        Console.SetForegroundColor((int)ConsoleColor.DarkRed);
    }

    [Fact]
    public void SetBackgroundColor_DoesNotThrow()
    {
        Console.SetBackgroundColor((int)ConsoleColor.Green);
    }

    [Fact]
    public void ResetColor_DoesNotThrow()
    {
        Console.ResetColor();
    }

    [Fact]
    public void ConsoleColor_Values()
    {
        Assert.Equal(0, (int)ConsoleColor.Black);
        Assert.Equal(7, (int)ConsoleColor.Gray);
        Assert.Equal(4, (int)ConsoleColor.DarkRed);
        Assert.Equal(10, (int)ConsoleColor.Green);
        Assert.Equal(14, (int)ConsoleColor.Yellow);
        Assert.Equal(11, (int)ConsoleColor.Cyan);
        Assert.Equal(15, (int)ConsoleColor.White);
    }
}