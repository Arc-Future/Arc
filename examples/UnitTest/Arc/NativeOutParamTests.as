namespace UnitTest.Arc;

using Arc;
using Arc.QIF;

/// <summary>
/// FFI out 参数 marshal（RFC 027 M2）：libc.frexp → out int 指数写回。
/// </summary>
public class NativeOutParamTests
{
    [Fact]
    public void Frexp_OutExp_WritesBack()
    {
        double val = 3.14;
        int exp = 0;
        double frac = libc.frexp(val, out exp);
        Assert.Equal(2, exp);
        Assert.True(frac > 0.7 && frac < 0.9);
    }
}
