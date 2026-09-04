// Thread — 显式操作系统线程（RFC 009 §7.1 / M5.5）
namespace Arc.Threading {

/// <summary>线程优先级——对齐 C# System.Threading.ThreadPriority。</summary>
public enum ThreadPriority {
    Lowest,
    BelowNormal,
    Normal,
    AboveNormal,
    Highest,
}

/// <summary>
/// 显式操作系统线程（1:1 模型，非 green thread / virtual thread）。
/// 线程在 Start() 调用时创建并立即执行，Join() 等待结束。
///
/// 此声明为 stub；new(Action) 在 emit_new.rs 拦截为 @rt_thread_handle_create，
/// Start()/Join()/IsAlive 等方法走 receiver_type 匹配。
/// 静态 Sleep 走 func 形式拦截；CurrentThread/ManagedThreadId 静态属性由 MIR
/// 降为 Call `"Thread.{Prop}"`，codegen try_emit_builtin_static → rt_thread_*。
/// </summary>
public class Thread {
    /// <summary>创建线程（不立即启动，需调用 Start()）。</summary>
    [Builtin(ABI = "rt_thread_create")]
    public Thread(Action start) {}

    /// <summary>线程是否存活（Start 后、Join 前为 true）。</summary>
    // 以 auto-property 书写（无访问器体），codegen 拦截 get_IsAlive → rt_thread_handle_is_alive。
    [Builtin(ABI = "rt_thread_is_alive")]
    public bool IsAlive { get; }

    // ── 线程属性 ──

    // Name/Priority/IsBackground/ThreadState 当前未被 codegen 拦截（无对应
    // rt_thread_* ABI handler），须保留 custom 访问器体，避免访问时链接失败。

    /// <summary>获取或设置线程名称（调试/诊断标识）。</summary>
    [Builtin(ABI = "rt_thread_get_name")]
    public string Name {
        get { return ""; }
        set { }
    }

    /// <summary>获取或设置线程优先级。</summary>
    [Builtin(ABI = "rt_thread_get_priority")]
    public ThreadPriority Priority {
        get { return ThreadPriority.Normal; }
        set { }
    }

    /// <summary>获取或设置是否为后台线程。（进程退出时后台线程被强制终止，前台线程阻止进程退出）。</summary>
    [Builtin(ABI = "rt_thread_is_background_get")]
    public bool IsBackground {
        get { return false; }
        set { }
    }

    /// <summary>获取线程的当前执行状态。</summary>
    [Builtin(ABI = "rt_thread_get_state")]
    public int ThreadState { get { return 0; } }

    // ── 生命周期 ──

    /// <summary>启动线程（创建 OS 线程并执行 Action）。</summary>
    [Builtin(ABI = "rt_thread_start")]
    public void Start() {}

    /// <summary>等待线程结束并释放 OS 句柄。</summary>
    [Builtin(ABI = "rt_thread_join")]
    public void Join() {}

    /// <summary>等待线程结束，限时 timeoutMs 毫秒。</summary>
    [Builtin(ABI = "rt_thread_join_timeout")]
    public bool Join(int timeoutMs) { return false; }

    /// <summary>中断处于 Wait/Sleep/Join 状态的线程。</summary>
    [Builtin(ABI = "rt_thread_interrupt")]
    public void Interrupt() {}

    // ── 静态方法 ──

    /// <summary>休眠当前线程。</summary>
    [Builtin(ABI = "rt_thread_sleep")]
    public static void Sleep(int milliseconds) {}

    /// <summary>获取当前线程句柄。</summary>
    [Builtin(ABI = "rt_thread_current")]
    public static Thread CurrentThread { get; }

    /// <summary>获取当前线程 ID（对齐 C# Thread.ManagedThreadId；ABI `rt_thread_current_id`）。</summary>
    [Builtin(ABI = "rt_thread_current_id")]
    public static int ManagedThreadId { get; }
}

} // namespace Arc.Threading
