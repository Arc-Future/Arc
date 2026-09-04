namespace UnitTest.QIF;

using Arc;
using Arc.QIF;

/// <summary>
/// 临时验证：纯同步测试套件，用于验证并行执行（Parallel.For）的正确性与加速。
/// 每个测试做一段 CPU 忙等，模拟真实计算负载。
/// </summary>
public class ParallelSyncCheck
{
    private static long Burn(int ms) {
        long sum = 0;
        long seed = 12345;
        for (int i = 0; i < ms * 200000; i = i + 1) {
            seed = seed * 1103515245 + 987654321;
            sum = sum + seed;
        }
        return sum;
    }

    [Fact]
    public void Burn_01() { Burn(60); Assert.True(true); }
    [Fact]
    public void Burn_02() { Burn(60); Assert.True(true); }
    [Fact]
    public void Burn_03() { Burn(60); Assert.True(true); }
    [Fact]
    public void Burn_04() { Burn(60); Assert.True(true); }
    [Fact]
    public void Burn_05() { Burn(60); Assert.True(true); }
    [Fact]
    public void Burn_06() { Burn(60); Assert.True(true); }
    [Fact]
    public void Burn_07() { Burn(60); Assert.True(true); }
    [Fact]
    public void Burn_08() { Burn(60); Assert.True(true); }
    [Fact]
    public void Burn_09() { Burn(60); Assert.True(true); }
    [Fact]
    public void Burn_10() { Burn(60); Assert.True(true); }
    [Fact]
    public void Burn_11() { Burn(60); Assert.True(true); }
    [Fact]
    public void Burn_12() { Burn(60); Assert.True(true); }
    [Fact]
    public void Burn_13() { Burn(60); Assert.True(true); }
    [Fact]
    public void Burn_14() { Burn(60); Assert.True(true); }
    [Fact]
    public void Burn_15() { Burn(60); Assert.True(true); }
    [Fact]
    public void Burn_16() { Burn(60); Assert.True(true); }
}