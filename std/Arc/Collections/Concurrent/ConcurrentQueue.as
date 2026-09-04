// RFC 024 M2/M7: Arc.Collections.Concurrent — 线程安全 FIFO 队列 facade。
// 对标 C# System.Collections.Concurrent.ConcurrentQueue<T>。
// 内部实现：Michael-Scott 无锁链表队列 + ABA 安全释放。
// M7：实现 IConcurrentCollection<T>（TryAdd/TryTake/CopyTo/ToList）。
namespace Arc.Collections.Concurrent;

/// <summary>线程安全 FIFO 队列。Michael-Scott 无锁实现。</summary>
public class ConcurrentQueue<T> : IConcurrentCollection<T> {
    private int _handle;

    [Builtin(ABI = "rt_concurrent_queue_create")]
    public ConcurrentQueue() { _handle = 0; }

    // ── 核心操作 ──

    /// <summary>入队。</summary>
    [Builtin(ABI = "rt_concurrent_queue_enqueue")]
    public void Enqueue(T item) { }

    /// <summary>尝试出队。队列空返回 false。</summary>
    [Builtin(ABI = "rt_concurrent_queue_try_dequeue")]
    public bool TryDequeue(out T item) { item = default(T); return false; }

    /// <summary>尝试查看队首（不出队）。</summary>
    [Builtin(ABI = "rt_concurrent_queue_try_peek")]
    public bool TryPeek(out T item) { item = default(T); return false; }

    // ── IConcurrentCollection ──

    /// <summary>尝试添加（≡ Enqueue，始终成功）。</summary>
    [Builtin(ABI = "rt_concurrent_queue_try_add")]
    public bool TryAdd(T item) { return false; }

    /// <summary>尝试取出（≡ TryDequeue）。</summary>
    [Builtin(ABI = "rt_concurrent_queue_try_take")]
    public bool TryTake(out T item) { item = default(T); return false; }

    // ── 集合属性 ──

    /// <summary>元素数（近似值）。</summary>
    [Builtin(ABI = "rt_concurrent_queue_count")]
    public int Count { get; }

    /// <summary>是否为空。</summary>
    [Builtin(ABI = "rt_concurrent_queue_is_empty")]
    public bool IsEmpty { get; }

    /// <summary>清空。</summary>
    [Builtin(ABI = "rt_concurrent_queue_clear")]
    public void Clear() { }

    // ── 快照 / 转换 ──

    /// <summary>拷贝为数组快照（非原子快照）。</summary>
    [Builtin(ABI = "rt_concurrent_queue_to_array")]
    public T[] ToArray() { return null; }

    /// <summary>复制到目标数组（自 index 起）。</summary>
    [Builtin(ABI = "rt_concurrent_queue_copy_to")]
    public void CopyTo(T[] array, int index) { }
}
