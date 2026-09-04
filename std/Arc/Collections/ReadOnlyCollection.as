namespace Arc.Collections;

/// <summary>只读集合包装器——对齐 C# System.Collections.ObjectModel.ReadOnlyCollection&lt;T&gt;。</summary>
/// <typeparam name="T">元素类型。</typeparam>
/// <remarks>
/// Stable 最小面：以 <c>List&lt;T&gt;</c> 为底层包装（非通用 <c>IList&lt;T&gt;</c> 分派——后者仍后置）。
/// 公开面：ctor(<c>List&lt;T&gt;</c>) / Count / 索引器 get / Contains / IndexOf / CopyTo
/// （<c>readonly_collection_e2e</c> 非 Skip）。底层列表突变对包装可见（与 C# 一致）。
/// <c>GetEnumerator</c> 为实现 <c>IReadOnlyList</c> 委托底层 List；Items / 任意 <c>IList</c> 仍后置——禁止静默 stub。
/// </remarks>
public class ReadOnlyCollection<T> : IReadOnlyList<T> {
    private List<T> _list;

    /// <summary>以指定列表初始化（只读包装；不拷贝）。</summary>
    public ReadOnlyCollection(List<T> list) {
        _list = list;
    }

    /// <summary>元素总数。</summary>
    public int Count {
        get { return _list.Count; }
    }

    // ── 索引器（C# this[int]，只读；RFC 007）──

    /// <summary>索引器 get：<c>collection[i]</c>。</summary>
    public T this[int index] {
        get { return _list[index]; }
    }

    /// <summary>判断是否包含。</summary>
    public bool Contains(T item) {
        return _list.Contains(item);
    }

    /// <summary>查找下标；未找到返回 -1。</summary>
    public int IndexOf(T item) {
        return _list.IndexOf(item);
    }

    /// <summary>复制到目标数组（自 <paramref name="arrayIndex"/> 起）。</summary>
    public void CopyTo(T[] array, int arrayIndex) {
        _list.CopyTo(array, arrayIndex);
    }

    /// <summary>返回枚举器（委托底层 List）。</summary>
    public IEnumerator<T> GetEnumerator() {
        return _list.GetEnumerator();
    }
}
