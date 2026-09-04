namespace Arc.Collections;

/// <summary>双向链表节点——对齐 C# System.Collections.Generic.LinkedListNode&lt;T&gt;。</summary>
/// <typeparam name="T">元素类型。</typeparam>
/// <remarks>
/// 运行时以不透明 <c>RtLinkedListNode*</c> 透传：<c>AddLast</c>/<c>First</c>/<c>Find</c>
/// 等返回的即节点句柄；属性访问走 <c>rt_linked_list_node_*</c>。
/// <c>List</c> 属性返回 runtime 链表句柄，非 Arc <c>LinkedList&lt;T&gt;</c> 包装——Stable 面不依赖它。
/// </remarks>
public class LinkedListNode<T> {
    // 布局占位：identity 模型下不读这些字段；节点内存由 runtime 拥有。
    private int _listHandle;
    private int _nodeHandle;

    /// <summary>节点值。</summary>
    [Builtin(ABI = "rt_linked_list_node_value")]
    public T Value { get; }

    /// <summary>前驱节点。为链表首节点时返回 null。</summary>
    [Builtin(ABI = "rt_linked_list_node_prev")]
    public LinkedListNode<T> Previous { get; }

    /// <summary>后继节点。为链表末节点时返回 null。</summary>
    [Builtin(ABI = "rt_linked_list_node_next")]
    public LinkedListNode<T> Next { get; }

    /// <summary>所属链表的 runtime 句柄（非 Arc 包装对象；Stable 面勿依赖）。</summary>
    [Builtin(ABI = "rt_linked_list_node_list")]
    public LinkedList<T> List { get; }
}
