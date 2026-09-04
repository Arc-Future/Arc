namespace UnitTest.Arc;

using Arc;
using Arc.Diagnostics;
using Arc.QIF;

/// <summary>
/// Arc.Diagnostics.Stopwatch 单元测试（非 Fact-Skip）。
/// 对齐 C# System.Diagnostics.Stopwatch 核心面；计时只认 Stopwatch。
/// </summary>
public class StopwatchTests
{
    [Fact]
    public void Frequency_Positive()
    {
        long freq = Stopwatch.Frequency;
        Assert.True(freq > 0);
    }

    [Fact]
    public void IsHighResolution_True()
    {
        Assert.True(Stopwatch.IsHighResolution);
    }

    [Fact]
    public void GetTimestamp_Monotonic()
    {
        long a = Stopwatch.GetTimestamp();
        long b = Stopwatch.GetTimestamp();
        Assert.True(b >= a);
    }

    [Fact]
    public void StartNew_IsRunning()
    {
        Stopwatch sw = Stopwatch.StartNew();
        Assert.True(sw.IsRunning);
        sw.Stop();
        Assert.True(!sw.IsRunning);
    }

    [Fact]
    public void Elapsed_AdvancesWhileRunning()
    {
        Stopwatch sw = Stopwatch.StartNew();
        long startTs = Stopwatch.GetTimestamp();
        long freq = Stopwatch.Frequency;
        // Busy-wait ~5ms of timer ticks (no Thread.Sleep dependency).
        long target = startTs + freq / 200;
        while (Stopwatch.GetTimestamp() < target) { }
        sw.Stop();
        Assert.True(sw.ElapsedTicks > 0);
        Assert.True(sw.ElapsedMilliseconds >= 0);
        TimeSpan elapsed = sw.Elapsed;
        Assert.True(elapsed.Ticks > 0);
    }

    [Fact]
    public void Reset_ClearsElapsed()
    {
        Stopwatch sw = Stopwatch.StartNew();
        long startTs = Stopwatch.GetTimestamp();
        long freq = Stopwatch.Frequency;
        long target = startTs + freq / 500;
        while (Stopwatch.GetTimestamp() < target) { }
        sw.Stop();
        Assert.True(sw.ElapsedTicks > 0);
        sw.Reset();
        Assert.True(!sw.IsRunning);
        long zero = 0;
        Assert.Equal(zero, sw.ElapsedTicks);
        Assert.Equal(zero, sw.ElapsedMilliseconds);
    }

    [Fact]
    public void Restart_RunningWithFreshElapsed()
    {
        Stopwatch sw = Stopwatch.StartNew();
        long startTs = Stopwatch.GetTimestamp();
        long freq = Stopwatch.Frequency;
        long target = startTs + freq / 500;
        while (Stopwatch.GetTimestamp() < target) { }
        sw.Stop();
        long before = sw.ElapsedTicks;
        Assert.True(before > 0);
        sw.Restart();
        Assert.True(sw.IsRunning);
        // Immediately after Restart, elapsed should be near zero (not cumulative).
        Assert.True(sw.ElapsedTicks < before);
        sw.Stop();
    }

    [Fact]
    public void Stop_AccumulatesAcrossSegments()
    {
        Stopwatch sw = new Stopwatch();
        sw.Start();
        long t0 = Stopwatch.GetTimestamp();
        long freq = Stopwatch.Frequency;
        while (Stopwatch.GetTimestamp() < t0 + freq / 500) { }
        sw.Stop();
        long first = sw.ElapsedTicks;
        sw.Start();
        long t1 = Stopwatch.GetTimestamp();
        while (Stopwatch.GetTimestamp() < t1 + freq / 500) { }
        sw.Stop();
        Assert.True(sw.ElapsedTicks > first);
    }
}
