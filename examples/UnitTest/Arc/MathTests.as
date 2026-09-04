namespace UnitTest.Arc;

using Arc;
using Arc.QIF;

/// <summary>
/// Arc.Math 诚实子集单元测试（非 Fact-Skip）。
/// 覆盖常量、舍入（含负向）、钳制（double/int/long）、符号、
/// int/long Min/Max、libm 反三角/双曲、对数族、
/// CopySign/Cbrt/Hypot/IEEERemainder。
/// </summary>
public class MathTests
{
    [Fact]
    public void Sqrt_PerfectSquare()
    {
        double result = Math.Sqrt(16.0);
        Assert.True(result > 3.999 && result < 4.001);
    }

    [Fact]
    public void Sqrt_Zero()
    {
        double result = Math.Sqrt(0.0);
        Assert.True(result == 0.0);
    }

    [Fact]
    public void Abs_Double_Positive()
    {
        double result = Math.Abs(3.14);
        Assert.True(result > 3.139 && result < 3.141);
    }

    [Fact]
    public void Abs_Double_Negative()
    {
        double result = Math.Abs(-3.14);
        Assert.True(result > 3.139 && result < 3.141);
    }

    [Fact]
    public void Abs_Int_Negative()
    {
        int result = Math.Abs(-5);
        Assert.Equal(5, result);
    }

    [Fact]
    public void Abs_Int_Zero()
    {
        int result = Math.Abs(0);
        Assert.Equal(0, result);
    }

    [Fact]
    public void Abs_Long_Negative()
    {
        long n = -9;
        long result = Math.Abs(n);
        long expected = 9;
        Assert.Equal(expected, result);
    }

    [Fact]
    public void Max()
    {
        double result = Math.Max(3.0, 7.0);
        Assert.True(result > 6.999 && result < 7.001);
    }

    [Fact]
    public void Min()
    {
        double result = Math.Min(3.0, 7.0);
        Assert.True(result > 2.999 && result < 3.001);
    }

    [Fact]
    public void Min_Int()
    {
        int r = Math.Min(3, 7);
        Assert.Equal(3, r);
    }

    [Fact]
    public void Max_Int()
    {
        int r = Math.Max(3, 7);
        Assert.Equal(7, r);
    }

    [Fact]
    public void Min_Max_Long()
    {
        long a = -9;
        long b = 4;
        long mn = Math.Min(a, b);
        long mx = Math.Max(a, b);
        long expectedMin = -9;
        long expectedMax = 4;
        Assert.Equal(expectedMin, mn);
        Assert.Equal(expectedMax, mx);
    }

    [Fact]
    public void Sin_Zero()
    {
        double result = Math.Sin(0.0);
        Assert.True(result == 0.0);
    }

    [Fact]
    public void Cos_Zero()
    {
        double result = Math.Cos(0.0);
        Assert.True(result > 0.999 && result < 1.001);
    }

    [Fact]
    public void Tan_Zero()
    {
        double result = Math.Tan(0.0);
        Assert.True(result > -0.001 && result < 0.001);
    }

    [Fact]
    public void Asin_Acos_Atan_Atan2()
    {
        double asin0 = Math.Asin(0.0);
        double acos1 = Math.Acos(1.0);
        double atan0 = Math.Atan(0.0);
        double atan2_q1 = Math.Atan2(1.0, 1.0);
        double quarterPi = Math.PI / 4.0;
        Assert.True(asin0 > -0.001 && asin0 < 0.001);
        Assert.True(acos1 > -0.001 && acos1 < 0.001);
        Assert.True(atan0 > -0.001 && atan0 < 0.001);
        Assert.True(atan2_q1 > quarterPi - 0.001 && atan2_q1 < quarterPi + 0.001);
    }

    [Fact]
    public void Sinh_Cosh_Tanh_Zero()
    {
        double sh = Math.Sinh(0.0);
        double ch = Math.Cosh(0.0);
        double th = Math.Tanh(0.0);
        Assert.True(sh > -0.001 && sh < 0.001);
        Assert.True(ch > 0.999 && ch < 1.001);
        Assert.True(th > -0.001 && th < 0.001);
    }

    [Fact]
    public void Pow_Basic()
    {
        double result = Math.Pow(2.0, 3.0);
        Assert.True(result > 7.999 && result < 8.001);
    }

    [Fact]
    public void Pow_ZeroExponent()
    {
        double result = Math.Pow(5.0, 0.0);
        Assert.True(result > 0.999 && result < 1.001);
    }

    [Fact]
    public void Log_E()
    {
        double e = Math.Exp(1.0);
        double result = Math.Log(e);
        Assert.True(result > 0.999 && result < 1.001);
    }

    [Fact]
    public void Log10_Hundred()
    {
        double result = Math.Log10(100.0);
        Assert.True(result > 1.999 && result < 2.001);
    }

    [Fact]
    public void Log2_Eight()
    {
        double result = Math.Log2(8.0);
        Assert.True(result > 2.999 && result < 3.001);
    }

    [Fact]
    public void Exp_Zero()
    {
        double result = Math.Exp(0.0);
        Assert.True(result > 0.999 && result < 1.001);
    }

    [Fact]
    public void Floor_Ceiling_Round_Truncate()
    {
        Assert.True(Math.Floor(3.7) > 2.999 && Math.Floor(3.7) < 3.001);
        Assert.True(Math.Ceiling(3.2) > 3.999 && Math.Ceiling(3.2) < 4.001);
        Assert.True(Math.Round(3.5) > 3.999 && Math.Round(3.5) < 4.001);
        Assert.True(Math.Truncate(3.9) > 2.999 && Math.Truncate(3.9) < 3.001);
        Assert.True(Math.Truncate(-3.9) > -3.001 && Math.Truncate(-3.9) < -2.999);
    }

    [Fact]
    public void Floor_Ceiling_Negative()
    {
        // Floor toward -∞；Ceiling toward +∞
        Assert.True(Math.Floor(-3.2) > -4.001 && Math.Floor(-3.2) < -3.999);
        Assert.True(Math.Ceiling(-3.2) > -3.001 && Math.Ceiling(-3.2) < -2.999);
        Assert.True(Math.Floor(-3.0) > -3.001 && Math.Floor(-3.0) < -2.999);
        Assert.True(Math.Ceiling(-3.0) > -3.001 && Math.Ceiling(-3.0) < -2.999);
    }

    [Fact]
    public void Sign_Int_And_Double()
    {
        int sn = Math.Sign(-5);
        int sz = Math.Sign(0);
        int sp = Math.Sign(5);
        int sdn = Math.Sign(-2.5);
        int sdz = Math.Sign(0.0);
        int sdp = Math.Sign(2.5);
        Assert.Equal(-1, sn);
        Assert.Equal(0, sz);
        Assert.Equal(1, sp);
        Assert.Equal(-1, sdn);
        Assert.Equal(0, sdz);
        Assert.Equal(1, sdp);
    }

    [Fact]
    public void Clamp_Basic()
    {
        double lo = Math.Clamp(-1.0, 0.0, 10.0);
        double mid = Math.Clamp(5.0, 0.0, 10.0);
        double hi = Math.Clamp(20.0, 0.0, 10.0);
        Assert.True(lo > -0.001 && lo < 0.001);
        Assert.True(mid > 4.999 && mid < 5.001);
        Assert.True(hi > 9.999 && hi < 10.001);
    }

    [Fact]
    public void Clamp_Int_And_Long()
    {
        int ilo = Math.Clamp(-5, 0, 10);
        int imid = Math.Clamp(5, 0, 10);
        int ihi = Math.Clamp(20, 0, 10);
        Assert.Equal(0, ilo);
        Assert.Equal(5, imid);
        Assert.Equal(10, ihi);

        long a = -5;
        long b = 5;
        long c = 20;
        long lo = 0;
        long hi = 10;
        long llo = Math.Clamp(a, lo, hi);
        long lmid = Math.Clamp(b, lo, hi);
        long lhi = Math.Clamp(c, lo, hi);
        long expectedLo = 0;
        long expectedMid = 5;
        long expectedHi = 10;
        Assert.Equal(expectedLo, llo);
        Assert.Equal(expectedMid, lmid);
        Assert.Equal(expectedHi, lhi);
    }

    [Fact]
    public void Constants_PI_E()
    {
        Assert.True(Math.PI > 3.1415 && Math.PI < 3.1416);
        Assert.True(Math.E > 2.7182 && Math.E < 2.7183);
    }

    [Fact]
    public void Fma_Basic()
    {
        double result = Math.Fma(2.0, 3.0, 4.0);
        Assert.True(result > 9.999 && result < 10.001);
    }

    [Fact]
    public void CopySign_MagnitudeAndSign()
    {
        double neg = Math.CopySign(3.5, -1.0);
        double pos = Math.CopySign(-3.5, 1.0);
        Assert.True(neg < -3.499 && neg > -3.501);
        Assert.True(pos > 3.499 && pos < 3.501);
    }

    [Fact]
    public void Cbrt_PerfectCube()
    {
        double r = Math.Cbrt(8.0);
        Assert.True(r > 1.999 && r < 2.001);
        double neg = Math.Cbrt(-8.0);
        Assert.True(neg < -1.999 && neg > -2.001);
    }

    [Fact]
    public void Hypot_345()
    {
        double r = Math.Hypot(3.0, 4.0);
        Assert.True(r > 4.999 && r < 5.001);
    }

    [Fact]
    public void IEEERemainder_Basic()
    {
        // IEEE remainder(5, 3) = -1（最近偶数商；非 fmod 的 2）
        double r = Math.IEEERemainder(5.0, 3.0);
        Assert.True(r > -1.001 && r < -0.999);
        double z = Math.IEEERemainder(4.0, 2.0);
        Assert.True(z > -0.001 && z < 0.001);
    }
}
