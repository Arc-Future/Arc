namespace UnitTest.Arc;

using Arc;
using Arc.QIF;

/// <summary>
/// FFI NativeInterop 单元测试：验证 .ani 契约调用 libc 函数。
/// `libc` 模块由 loader 自动扫描 native/libc.ani 注册为 StaticClass，
/// 调用语法与普通静态方法一致。
/// </summary>
public class NativeInteropTests
{
    [Fact]
    public void Libc_Puts_ReturnsInt()
    {
        var ret = libc.puts("QIF FFI test: libc.puts");
        // puts 返回非负整数表示成功
        Assert.True(ret >= 0);
    }

    [Fact]
    public void Libc_Puts_NoException()
    {
        libc.puts("Native contract verified via QIF.");
        Assert.True(true);
    }
}
