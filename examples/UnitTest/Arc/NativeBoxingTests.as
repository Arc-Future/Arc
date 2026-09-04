namespace UnitTest.Arc;

using Arc;
using Arc.QIF;

/// <summary>
/// FFI object 装箱/拆箱（RFC 030 / 027 M3）：void* ↔ object via memcmp/memcpy。
/// </summary>
public class NativeBoxingTests
{
    [Fact]
    public void Memcmp_ZeroBytes_BoxSmoke()
    {
        int x = 42;
        int cmp = libc.memcmp(x, x, 0);
        Assert.Equal(0, cmp);
    }

    [Fact]
    public void Memcpy_BoxUnbox_Roundtrip()
    {
        int x = 42;
        object boxed = libc.memcpy(x, x, 0);
        int y = (int)boxed;
        Assert.Equal(42, y);
    }
}
