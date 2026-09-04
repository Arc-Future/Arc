// RFC 024 M3/M7: Arc.Collections.Concurrent — 线程安全无序集合 facade。
// 对标 C# System.Collections.Concurrent.ConcurrentBag<T>。
// 内部实现：per-worker 本地链 + Work-Stealing Deque steal。
// M7：实现 IConcurrentCollection<T>。
namespace Arc.Collections.Concurrent;

/// <summary>线程安全无序集合。per-worker 本地链 + Work-Stealing steal。</summary>
public class ConcurrentBag<T> : IConcurrentCollection<T> {
    private int _handle;

    [Builtin(ABI = "rt_concurrent_bag_create")]
    public ConcurrentBag() { _handle = 0; }

    // ── 核心操作 ──

    /// <summary>添加元素（本线程 fast path，零锁）。</summary>
    [Builtin(ABI = "rt_concurrent_bag_add")]
    public void Add(T item) { }

    /// <summary>尝试取出。优先本线程链（零锁），否则 steal。</summary>
    [Builtin(ABI = "rt_concurrent_bag_try_take")]
    public bool TryTake(out T item) { item = default(T); return false; }

    /// <summary>尝试查看（不出队）。</summary>
    [Builtin(ABI = "rt_concurrent_bag_try_peek")]
    public bool TryPeek(out T item) { item = default(T); return false; }

    // ── IConcurrentCollection ──

    /// <summary>尝试添加（≡ Add，始终成功）。</summary>
    [Builtin(ABI = "rt_concurrent_bag_try_add")]
    public bool TryAdd(T item) { return false; }

    // ── 集合属性 ──

    /// <summary>元素数（近似值）。</summary>
    [Builtin(ABI = "rt_concurrent_bag_count")]
    public int Count { get; }

    /// <summary>是否为空。</summary>
    [Builtin(ABI = "rt_concurrent_bag_is_empty")]
    public bool IsEmpty { get; }

    /// <summary>清空。</summary>
    [Builtin(ABI = "rt_concurrent_bag_clear")]
    public void Clear() { }

    // ── 快照 / 转换 ──

    /// <summary>拷贝为数组快照（非原子快照）。</summary>
    [Builtin(ABI = "rt_concurrent_bag_to_array")]
    public T[] ToArray() { return null; }

    /// <summary>复制到目标数组（自 index 起）。</summary>
    [Builtin(ABI = "rt_concurrent_bag_copy_to")]
    public void CopyTo(T[] array, int index) { }
}
