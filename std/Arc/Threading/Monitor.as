// Monitor — 条件变量同步（RFC 009 §7.2 / M5.5）
namespace Arc.Threading {

/// <summary>
/// 监视器——基于 Lock 对象的条件变量同步。仅作用于 Lock 类实例
/// （非任意 object，与 C# Monitor 不同），避免为所有 class 实例
/// 追加 sync-block 头开销（零开销抽象 + 显式优于隐式）。
///
/// 此声明为 stub，全部为静态方法；codegen 拦截后直接发射 rt_monitor_* ABI。
/// </summary>
public class Monitor {
    /// <summary>获取锁（进入临界区）。</summary>
    [Builtin(ABI = "rt_monitor_enter")]
    public static void Enter(Lock obj) {}
    /// <summary>释放锁（退出临界区）。</summary>
    [Builtin(ABI = "rt_monitor_exit")]
    public static void Exit(Lock obj) {}
    /// <summary>尝试获取锁（非阻塞）。</summary>
    /// <returns>true 表示成功获取。</returns>
    [Builtin(ABI = "rt_monitor_try_enter")]
    public static bool TryEnter(Lock obj) { return false; }
    /// <summary>尝试获取锁，带超时。</summary>
    [Builtin(ABI = "rt_monitor_try_enter_timeout")]
    public static bool TryEnter(Lock obj, int milliseconds) { return false; }
    /// <summary>释放锁并等待 Pulse 唤醒。</summary>
    [Builtin(ABI = "rt_monitor_wait")]
    public static void Wait(Lock obj) {}
    /// <summary>释放锁并等待 Pulse 唤醒，带超时（毫秒）。</summary>
    /// <returns>true 在被 Pulse 前返回；false 超时。</returns>
    [Builtin(ABI = "rt_monitor_wait_timeout")]
    public static bool Wait(Lock obj, int milliseconds) { return false; }
    /// <summary>唤醒一个等待者。</summary>
    [Builtin(ABI = "rt_monitor_pulse")]
    public static void Pulse(Lock obj) {}
    /// <summary>唤醒所有等待者。</summary>
    [Builtin(ABI = "rt_monitor_pulse_all")]
    public static void PulseAll(Lock obj) {}
}

} // namespace Arc.Threading
