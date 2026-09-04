// RFC 024 M4/M7: Arc.Collections.Concurrent — 线程安全 LIFO 栈 facade。
// 对标 C# System.Collections.Concurrent.ConcurrentStack<T>。
// 内部实现：Treiber stack 无锁 + tagged pointer ABA 防护。
// M7：实现 IConcurrentCollection<T>。
namespace Arc.Collections.Concurrent;

/// <summary>线程安全 LIFO 栈。Treiber stack 无锁实现。</summary>
public class ConcurrentStack<T> : IConcurrentCollection<T> {
    private int _handle;

    [Builtin(ABI = "rt_concurrent_stack_create")]
    public ConcurrentStack() { _handle = 0; }

    // ── 核心操作 ──

    /// <summary>压入单个元素。</summary>
    [Builtin(ABI = "rt_concurrent_stack_push")]
    public void Push(T item) { }

    /// <summary>尝试弹出。栈空返回 false。</summary>
    [Builtin(ABI = "rt_concurrent_stack_try_pop")]
    public bool TryPop(out T item) { item = default(T); return false; }

    /// <summary>尝试查看栈顶（不弹出）。</summary>
    [Builtin(ABI = "rt_concurrent_stack_try_peek")]
    public bool TryPeek(out T item) { item = default(T); return false; }

    // ── 批量：C ABI `rt_concurrent_stack_*_range`（void**）由 concurrent_stack_e2e 覆盖。
    // Arc 面不挂 PushRange/TryPopRange——Builtin facade 的非 [Builtin] 方法目前走
    // 空 stub（linkonce），禁止半成品挂面；用户用 Push/TryPop 循环即可。

    // ── IConcurrentCollection ──

    /// <summary>尝试添加（≡ Push，始终成功）。</summary>
    [Builtin(ABI = "rt_concurrent_stack_try_add")]
    public bool TryAdd(T item) { return false; }

    /// <summary>尝试取出（≡ TryPop）。</summary>
    [Builtin(ABI = "rt_concurrent_stack_try_take")]
    public bool TryTake(out T item) { item = default(T); return false; }

    // ── 集合属性 ──

    /// <summary>元素数（近似值）。</summary>
    [Builtin(ABI = "rt_concurrent_stack_count")]
    public int Count { get; }

    /// <summary>是否为空。</summary>
    [Builtin(ABI = "rt_concurrent_stack_is_empty")]
    public bool IsEmpty { get; }

    /// <summary>清空。</summary>
    [Builtin(ABI = "rt_concurrent_stack_clear")]
    public void Clear() { }

    // ── 快照 / 转换 ──

    /// <summary>拷贝为数组快照（栈顶 → 数组[0]）。</summary>
    [Builtin(ABI = "rt_concurrent_stack_to_array")]
    public T[] ToArray() { return null; }

    /// <summary>复制到目标数组（自 index 起）。</summary>
    [Builtin(ABI = "rt_concurrent_stack_copy_to")]
    public void CopyTo(T[] array, int index) { }
}
