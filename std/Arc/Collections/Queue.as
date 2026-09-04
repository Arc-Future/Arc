namespace Arc.Collections;

/// <summary>泛型队列（FIFO）——对齐 C# System.Collections.Generic.Queue&lt;T&gt;。</summary>
/// <typeparam name="T">元素类型。</typeparam>
/// <remarks>
/// 核心操作由 runtime ABI (<c>rt_queue_*</c>) 提供。
/// 成熟度：最小可测（须非 Skip e2e；见 RFC 009）。
/// </remarks>
public class Queue<T> {
    // RFC 050：句柄字段必须 **8B 宽（long）**——runtime ctor 拦截写 8B 指针到
    // offset 16，若声明为 int（4B）且 ctor body 重写 `_handle = 0`，低 4B 被
    // 清零造成 handle 撕裂（高位残留 → 错位指针 → Peek/Enqueue 解引用崩，
    // channels backpressure 0xC0000005 实证）。ctor body 不再重写：
    // handle 由 runtime 拦截一次性写入。
    private long _handle;

    // ── 构造函数 ──

    /// <summary>构造空队列。</summary>
    public Queue() {
    }

    /// <summary>构造指定初始容量的队列。</summary>
    public Queue(int capacity) {
    }

    // ── 核心操作 ──

    /// <summary>入队——将元素添加到队尾。</summary>
    [Builtin(ABI = "rt_queue_enqueue")]
    public void Enqueue(T item) { }

    /// <summary>出队——移除并返回队首元素。队空返回 default(T)。</summary>
    [Builtin(ABI = "rt_queue_dequeue")]
    public T Dequeue() {
        return 0;
    }

    /// <summary>尝试出队。队空返回 false。</summary>
    [Builtin(ABI = "rt_queue_dequeue")]
    public bool TryDequeue(out T item) {
        item = default(T);
        return false;
    }

    /// <summary>查看队首元素但不移除。队空返回 default(T)。</summary>
    [Builtin(ABI = "rt_queue_peek")]
    public T Peek() {
        return 0;
    }

    /// <summary>尝试查看队首。队空返回 false。</summary>
    [Builtin(ABI = "rt_queue_peek")]
    public bool TryPeek(out T item) {
        item = default(T);
        return false;
    }

    // ── 集合属性 ──

    /// <summary>元素总数。</summary>
    [Builtin(ABI = "rt_queue_count")]
    public int Count { get; }

    /// <summary>清空所有元素。</summary>
    [Builtin(ABI = "rt_queue_clear")]
    public void Clear() { }

    /// <summary>判断是否包含指定元素。</summary>
    /// <remarks>内部通过循环缓冲区扫描实现，O(n) 复杂度，不修改队列。</remarks>
    [Builtin(ABI = "rt_queue_contains")]
    public bool Contains(T item) {
        return false;
    }

    /// <summary>拷贝为数组（队首 → 数组[0]，不修改队列内容）。</summary>
    [Builtin(ABI = "rt_queue_to_array")]
    public T[] ToArray() {
        return 0;
    }
}
