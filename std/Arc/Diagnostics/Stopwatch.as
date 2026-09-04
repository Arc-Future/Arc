// Arc.Diagnostics.Stopwatch — 高精度时间间隔测量（对标 C# System.Diagnostics.Stopwatch）。
//
// 单一惯用法（RFC 002）：性能/间隔计时只认本类型；不再经 Environment.TickCount*。
// 计时源：Windows QueryPerformanceCounter；POSIX CLOCK_MONOTONIC（纳秒）。
// IsHighResolution 恒为 true（诚实：两平台均走高精度时钟，无低精度降级路径）。

namespace Arc.Diagnostics;

/// <summary>
/// 高精度时间间隔测量器，对齐 C# <c>System.Diagnostics.Stopwatch</c>。
///
/// <b>C ABI</b>（经 <c>rt_resources</c>）：
///   - <see cref="GetTimestamp"/> → <c>rt_stopwatch_get_timestamp()</c>
///   - <see cref="Frequency"/> → <c>rt_stopwatch_frequency()</c>
///   - <see cref="IsHighResolution"/> → <c>rt_stopwatch_is_high_resolution()</c>
///
/// <b>刻度约定</b>：
///   - <see cref="ElapsedTicks"/> = 计时器原始 ticks（非 <see cref="TimeSpan"/> 的 100ns ticks）
///   - <see cref="Elapsed"/> / <see cref="ElapsedMilliseconds"/> 经 Frequency 换算为
///     TimeSpan 刻度（每秒 10_000_000）
///
/// <code>
/// Stopwatch sw = Stopwatch.StartNew();
/// // … work …
/// sw.Stop();
/// long ms = sw.ElapsedMilliseconds;
/// TimeSpan elapsed = sw.Elapsed;
/// </code>
/// </summary>
public class Stopwatch {
    private long _elapsed;
    private long _startTimeStamp;
    private bool _isRunning;

    /// <summary>创建未启动的计时器。</summary>
    public Stopwatch() {
        _elapsed = 0;
        _startTimeStamp = 0;
        _isRunning = false;
    }

    /// <summary>计时器频率（每秒 ticks）。Windows = QPC 频率；POSIX = 1_000_000_000。</summary>
    public static long Frequency {
        get { return rt_resources.rt_stopwatch_frequency(); }
    }

    /// <summary>是否使用高精度计时器。当前实现两平台均为 true。</summary>
    public static bool IsHighResolution {
        get { return rt_resources.rt_stopwatch_is_high_resolution() != 0; }
    }

    /// <summary>读取当前计时器原始时间戳。</summary>
    public static long GetTimestamp() {
        return rt_resources.rt_stopwatch_get_timestamp();
    }

    /// <summary>创建并立即启动计时器。</summary>
    public static Stopwatch StartNew() {
        Stopwatch sw = new Stopwatch();
        sw.Start();
        return sw;
    }

    /// <summary>是否正在计时。</summary>
    public bool IsRunning {
        get { return _isRunning; }
    }

    /// <summary>开始或恢复计时。已在运行时无操作。</summary>
    public void Start() {
        if (!_isRunning) {
            _startTimeStamp = Stopwatch.GetTimestamp();
            _isRunning = true;
        }
    }

    /// <summary>停止计时，累加自上次 Start 以来的间隔。未在运行时无操作。</summary>
    public void Stop() {
        if (_isRunning) {
            long end = Stopwatch.GetTimestamp();
            long delta = end - _startTimeStamp;
            _elapsed = _elapsed + delta;
            _isRunning = false;
            if (_elapsed < 0) {
                _elapsed = 0;
            }
        }
    }

    /// <summary>清零并停止。</summary>
    public void Reset() {
        _elapsed = 0;
        _isRunning = false;
        _startTimeStamp = 0;
    }

    /// <summary>清零并重新开始计时。</summary>
    public void Restart() {
        _elapsed = 0;
        _startTimeStamp = Stopwatch.GetTimestamp();
        _isRunning = true;
    }

    /// <summary>已流逝的计时器原始 ticks（含当前运行段）。</summary>
    public long ElapsedTicks {
        get { return this.GetRawElapsedTicks(); }
    }

    /// <summary>已流逝的整毫秒数。</summary>
    public long ElapsedMilliseconds {
        get {
            long dateTimeTicks = this.GetElapsedDateTimeTicks();
            return dateTimeTicks / 10000;
        }
    }

    /// <summary>已流逝时间，以 <see cref="TimeSpan"/> 表示。</summary>
    public TimeSpan Elapsed {
        get { return new TimeSpan(this.GetElapsedDateTimeTicks()); }
    }

    private long GetRawElapsedTicks() {
        long timeElapsed = _elapsed;
        if (_isRunning) {
            long now = Stopwatch.GetTimestamp();
            timeElapsed = timeElapsed + (now - _startTimeStamp);
        }
        return timeElapsed;
    }

    /// <summary>
    /// 将计时器 ticks 换算为 TimeSpan ticks（100ns）。
    /// 使用分段乘法避免 long 溢出：seconds*10M + (rem*10M)/freq。
    /// </summary>
    private long GetElapsedDateTimeTicks() {
        long rawTicks = this.GetRawElapsedTicks();
        long frequency = Stopwatch.Frequency;
        long seconds = rawTicks / frequency;
        long remaining = rawTicks - seconds * frequency;
        return seconds * 10000000 + (remaining * 10000000) / frequency;
    }
}
