// RFC 024 M5/M7: Arc.Collections.Concurrent — 阻塞式线程安全集合 facade。
// 对标 C# System.Collections.Concurrent.BlockingCollection<T>。
// 内部：IConcurrentCollection 底层（默认 ConcurrentQueue）+ 双 Semaphore。
// M7：create_with(inner, kind, …) + Arc 面 BlockingCollection(IConcurrentCollection, int)
//（codegen 按实参静态类型分派 Queue/Bag/Stack；见 blocking_collection_pcc_ctor_e2e）。
namespace Arc.Collections.Concurrent;

/// <summary>阻塞式线程安全集合——生产者-消费者模式边界容器。</summary>
public class BlockingCollection<T> {
    private int _handle;

    // ── 构造函数 ──

    /// <summary>构造无界阻塞集合（底层 ConcurrentQueue）。</summary>
    public BlockingCollection() {
        _handle = 0;
    }

    /// <summary>构造指定容量上限的阻塞集合（0=无界；底层 ConcurrentQueue）。</summary>
    [Builtin(ABI = "rt_blocking_collection_create")]
    public BlockingCollection(int boundedCapacity) { _handle = 0; }

    /// <summary>
    /// 以既有 IConcurrentCollection 为底层构造阻塞集合。
    /// 最小可测：ConcurrentQueue / ConcurrentBag / ConcurrentStack；自定义并发集合未宣称。
    /// </summary>
    [Builtin(ABI = "rt_blocking_collection_create_with")]
    public BlockingCollection(IConcurrentCollection<T> collection, int boundedCapacity) {
        _handle = 0;
    }

    // ── 阻塞操作 ──

    /// <summary>添加元素。满时阻塞直到有空间或取消。</summary>
    [Builtin(ABI = "rt_blocking_collection_add")]
    public void Add(T item) { }

    /// <summary>取出元素。空时阻塞直到有元素或取消。</summary>
    [Builtin(ABI = "rt_blocking_collection_take")]
    public T Take() { return default(T); }

    // ── 非阻塞操作 ──

    /// <summary>尝试添加元素。满/已完成返回 false。</summary>
    [Builtin(ABI = "rt_blocking_collection_try_add")]
    public bool TryAdd(T item) { return false; }

    /// <summary>尝试取出元素。空/已完成返回 false。</summary>
    [Builtin(ABI = "rt_blocking_collection_try_take")]
    public bool TryTake(out T item) { item = default(T); return false; }

    // ── 超时操作 ──

    /// <summary>尝试添加元素，最多等待 millisecondsTimeout 毫秒。</summary>
    [Builtin(ABI = "rt_blocking_collection_try_add_to")]
    public bool TryAdd(T item, int millisecondsTimeout) { return false; }

    /// <summary>尝试取出元素，最多等待 millisecondsTimeout 毫秒。</summary>
    [Builtin(ABI = "rt_blocking_collection_try_take_to")]
    public bool TryTake(out T item, int millisecondsTimeout) { item = default(T); return false; }

    // ── 完成标记 ──

    /// <summary>标记添加完成。后续 Add 操作将抛出异常。</summary>
    [Builtin(ABI = "rt_blocking_collection_complete")]
    public void CompleteAdding() { }

    /// <summary>是否已调用 CompleteAdding。</summary>
    [Builtin(ABI = "rt_blocking_collection_is_adding_completed")]
    public bool IsAddingCompleted { get; }

    /// <summary>是否已完成（添加完成 + 集合空）。</summary>
    [Builtin(ABI = "rt_blocking_collection_is_completed")]
    public bool IsCompleted { get; }

    // ── 集合属性 ──

    /// <summary>当前元素数（近似值）。</summary>
    [Builtin(ABI = "rt_blocking_collection_count")]
    public int Count { get; }

    /// <summary>容量上限。无界集合返回 int.MaxValue（对标 .NET BoundedCapacity）。</summary>
    [Builtin(ABI = "rt_blocking_collection_bounded_capacity")]
    public int BoundedCapacity { get; }

    // ── 快照 / 转换 ──

    /// <summary>将当前元素拷贝到数组中。</summary>
    [Builtin(ABI = "rt_blocking_collection_copy_to")]
    public void CopyTo(T[] array, int index) { }

    /// <summary>拷贝为数组快照。</summary>
    [Builtin(ABI = "rt_blocking_collection_to_array")]
    public T[] ToArray() { return null; }
}
