namespace Arc.Collections;

/// <summary>双向链表——对齐 C# System.Collections.Generic.LinkedList&lt;T&gt;。</summary>
/// <typeparam name="T">元素类型。</typeparam>
/// <remarks>
/// 内部由 runtime ABI (<c>rt_linked_list_*</c>) 管理。
/// 成熟度：Stable 最小面——ctor / AddFirst / AddLast / Count / First / Last /
/// Contains / Remove(T) / Clear / Find / 节点 Value·Previous·Next
/// （<c>linked_list_e2e</c> 非 Skip）。节点 <c>List</c> 属性与 Arc 包装对齐仍后置。
/// </remarks>
public class LinkedList<T> {
    private int _handle;

    /// <summary>构造空链表。</summary>
    [Builtin(ABI = "rt_linked_list_create")]
    public LinkedList() {
        _handle = 0;
    }

    // ── 节点操作 ──

    /// <summary>添加节点到末尾。</summary>
    [Builtin(ABI = "rt_linked_list_add_last")]
    public LinkedListNode<T> AddLast(T value) {
        return 0;
    }

    /// <summary>添加节点到开头。</summary>
    [Builtin(ABI = "rt_linked_list_add_first")]
    public LinkedListNode<T> AddFirst(T value) {
        return 0;
    }

    /// <summary>在指定节点后添加。</summary>
    [Builtin(ABI = "rt_linked_list_add_after")]
    public LinkedListNode<T> AddAfter(LinkedListNode<T> node, T value) {
        return 0;
    }

    /// <summary>在指定节点前添加。</summary>
    [Builtin(ABI = "rt_linked_list_add_before")]
    public LinkedListNode<T> AddBefore(LinkedListNode<T> node, T value) {
        return 0;
    }

    /// <summary>移除节点。</summary>
    [Builtin(ABI = "rt_linked_list_remove_node")]
    public void Remove(LinkedListNode<T> node) { }

    /// <summary>移除指定值首次出现。</summary>
    [Builtin(ABI = "rt_linked_list_remove")]
    public bool Remove(T value) {
        return false;
    }

    // ── 首/末 ──

    /// <summary>首个节点。</summary>
    [Builtin(ABI = "rt_linked_list_first")]
    public LinkedListNode<T> First { get; }

    /// <summary>末个节点。</summary>
    [Builtin(ABI = "rt_linked_list_last")]
    public LinkedListNode<T> Last { get; }

    // ── 集合属性 ──

    /// <summary>元素总数。</summary>
    [Builtin(ABI = "rt_linked_list_count")]
    public int Count { get; }

    /// <summary>清空所有节点。</summary>
    [Builtin(ABI = "rt_linked_list_clear")]
    public void Clear() { }

    /// <summary>查找指定值的首个节点。</summary>
    [Builtin(ABI = "rt_linked_list_find")]
    public LinkedListNode<T> Find(T value) {
        return 0;
    }

    /// <summary>查找指定值的末个节点。</summary>
    [Builtin(ABI = "rt_linked_list_find_last")]
    public LinkedListNode<T> FindLast(T value) {
        return 0;
    }

    /// <summary>判断是否包含指定值。</summary>
    [Builtin(ABI = "rt_linked_list_contains")]
    public bool Contains(T value) {
        return false;
    }
}
