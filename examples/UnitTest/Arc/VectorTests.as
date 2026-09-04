namespace UnitTest.Arc;

using Arc;
using Arc.QIF;

/// <summary>
/// Vector&lt;T, N&gt; SIMD（RFC 021 Phase 2）：Get/Set/Add/Sub/Mul/Fma 与宽度覆盖。
/// </summary>
public class VectorTests
{
    [Fact]
    public void Float4_Arithmetic()
    {
        Vector<float, 4> a = new Vector<float, 4>();
        Vector<float, 4> b = new Vector<float, 4>();
        a = Vector.Set(a, 0, 1.0);
        a = Vector.Set(a, 1, 2.0);
        a = Vector.Set(a, 2, 3.0);
        a = Vector.Set(a, 3, 4.0);
        b = Vector.Set(b, 0, 10.0);
        b = Vector.Set(b, 1, 20.0);
        b = Vector.Set(b, 2, 30.0);
        b = Vector.Set(b, 3, 40.0);

        Assert.True(Vector.Get(a, 0) == 1.0 && Vector.Get(a, 3) == 4.0);

        Vector<float, 4> sum = Vector.Add(a, b);
        Assert.True(Vector.Get(sum, 0) == 11.0 && Vector.Get(sum, 3) == 44.0);

        Vector<float, 4> diff = Vector.Sub(b, a);
        Assert.True(Vector.Get(diff, 0) == 9.0 && Vector.Get(diff, 3) == 36.0);

        Vector<float, 4> prod = Vector.Mul(a, a);
        Assert.True(Vector.Get(prod, 0) == 1.0 && Vector.Get(prod, 3) == 16.0);

        Vector<float, 4> fma = Vector.Fma(a, a, b);
        Assert.True(Vector.Get(fma, 0) == 11.0 && Vector.Get(fma, 3) == 56.0);
    }

    [Fact]
    public void Double8_And_Float16_Width()
    {
        Vector<double, 8> d = new Vector<double, 8>();
        d = Vector.Set(d, 0, 1.0);
        d = Vector.Set(d, 7, 8.0);
        Assert.True(Vector.Get(d, 0) == 1.0 && Vector.Get(d, 7) == 8.0);

        Vector<float, 16> w = new Vector<float, 16>();
        w = Vector.Set(w, 0, 5.0);
        w = Vector.Set(w, 15, 6.0);
        Assert.True(Vector.Get(w, 0) == 5.0 && Vector.Get(w, 15) == 6.0);
    }
}
