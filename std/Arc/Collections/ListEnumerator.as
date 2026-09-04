namespace Arc.Collections;

/// <summary>列表枚举器——对齐 C# List&lt;T&gt;.Enumerator。</summary>
/// <typeparam name="T">元素类型。</typeparam>
/// <remarks>
/// 实现 IEnumerator&lt;T&gt; 协议，支持 foreach 遍历。
/// 由 runtime ABI (rt_list_enumerator_*) 提供实现。
/// </remarks>
public class ListEnumerator<T> : IEnumerator<T> {
    private int _handle;

    public ListEnumerator() {
        _handle = 0;
    }

    [Builtin(ABI = "rt_list_enumerator_move_next")]
    public bool MoveNext() {
        return false;
    }

    [Builtin(ABI = "rt_list_enumerator_current")]
    public T Current { get; }
}
