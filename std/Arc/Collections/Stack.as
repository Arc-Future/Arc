namespace Arc.Collections;

/// <summary>泛型栈（LIFO）——对齐 C# System.Collections.Generic.Stack&lt;T&gt;。</summary>
/// <typeparam name="T">元素类型。</typeparam>
/// <remarks>
/// 实现由 runtime ABI (<c>rt_stack_*</c>) 提供。
/// 成熟度：最小可测（须非 Skip e2e；见 RFC 009）。
/// </remarks>
public class Stack<T> {
    private int _handle;

    // ── 构造函数 ──

    public Stack() {
        _handle = 0;
    }

    public Stack(int capacity) {
        _handle = 0;
    }

    // ── 核心操作 ──

    [Builtin(ABI = "rt_stack_push")]
    public void Push(T item) { }

    [Builtin(ABI = "rt_stack_pop")]
    public T Pop() {
        return 0;
    }

    [Builtin(ABI = "rt_stack_peek")]
    public T Peek() {
        return 0;
    }

    [Builtin(ABI = "rt_stack_try_pop")]
    public bool TryPop(out T item) {
        item = default(T);
        return false;
    }

    [Builtin(ABI = "rt_stack_try_peek")]
    public bool TryPeek(out T item) {
        item = default(T);
        return false;
    }

    // ── 集合属性 ──

    [Builtin(ABI = "rt_stack_count")]
    public int Count { get; }

    [Builtin(ABI = "rt_stack_clear")]
    public void Clear() { }

    [Builtin(ABI = "rt_stack_contains")]
    public bool Contains(T item) {
        return false;
    }

    [Builtin(ABI = "rt_stack_to_array")]
    public T[] ToArray() {
        return 0;
    }
}
