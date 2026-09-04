namespace UnitTest.Arc;

using Arc;
using Arc.QIF;
using Arc.Types;

/// <summary>
/// Arc.Types.Random Stable 最小面（非 Fact-Skip）。
/// LCG；非 CSPRNG。同 seed 可复现。NextBytes = <c>Next() % 256</c>（无 bitwise）。
/// </summary>
public class RandomTests
{
    [Fact]
    public void Next_Seeded_Deterministic()
    {
        Random a = new Random(42);
        Random b = new Random(42);
        Assert.Equal(a.Next(), b.Next());
        Assert.Equal(a.Next(100), b.Next(100));
        Assert.Equal(a.Next(10, 50), b.Next(10, 50));
    }

    [Fact]
    public void Next_MaxValue_InRange()
    {
        Random r = new Random(7);
        int i = 0;
        while (i < 20) {
            int v = r.Next(10);
            Assert.True(v >= 0);
            Assert.True(v < 10);
            i = i + 1;
        }
    }

    [Fact]
    public void NextDouble_Range()
    {
        Random r = new Random(3);
        double d = r.NextDouble();
        Assert.True(d >= 0.0);
        Assert.True(d < 1.0);
    }

    [Fact]
    public void NextBytes_Seeded_Match()
    {
        byte[] a = [0, 0, 0, 0, 0, 0, 0, 0];
        byte[] b = [0, 0, 0, 0, 0, 0, 0, 0];
        Random ra = new Random(42);
        Random rb = new Random(42);
        ra.NextBytes(a);
        rb.NextBytes(b);
        int i = 0;
        while (i < 8) {
            Assert.Equal((int)a[i], (int)b[i]);
            i = i + 1;
        }
    }

    [Fact]
    public void NextBytes_FillsBuffer()
    {
        byte[] buf = [0, 0, 0, 0];
        Random r = new Random(99);
        r.NextBytes(buf);
        Assert.Equal(4, buf.Length);
        // At least one non-zero across typical LCG draws (seed 99).
        int sum = (int)buf[0] + (int)buf[1] + (int)buf[2] + (int)buf[3];
        Assert.True(sum > 0);
    }
}
