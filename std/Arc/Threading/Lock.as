// Lock — 锁对象（RFC 009 §7.2 / M5.5）
namespace Arc.Threading {

/// <summary>
/// 锁对象——Monitor 与 lock 语句的目标类型。专用 Lock 类而非任意 object：
/// 避免为所有 class 实例追加 sync-block 头开销（零开销抽象 + 显式优于隐式）。
///
/// 此声明为 stub；new() 在 emit_new.rs 拦截为 @rt_lock_create()，
/// 返回包含 mutex + condvar 的 rt_monitor_obj*。
/// lock (myLock) { ... } 脱糖为 Monitor.Enter/Exit + try/finally
/// （typeck 展开；MIR/codegen 复用既有 Enter/Exit + TryFinally 路径）。
/// </summary>
public class Lock {
    /// <summary>创建锁对象（初始化 mutex + condvar）。</summary>
    [Builtin(ABI = "rt_lock_create")]
    public Lock() {}
}

} // namespace Arc.Threading
