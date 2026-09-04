namespace Arc.Collections;

/// <summary>可扩展集合包装——对齐 C# System.Collections.ObjectModel.Collection&lt;T&gt; 最小面。</summary>
/// <typeparam name="T">元素类型。</typeparam>
/// <remarks>
/// 内部委托具体 <c>List&lt;T&gt;</c>（非 <c>IList&lt;T&gt;</c> 接口字段——接口派发 / GetEnumerator 仍后置）。
/// 成熟度：Stable 最小面——ctor / Add / Contains / Remove / Count / 索引器 /
/// IndexOf / Insert / RemoveAt / Clear / CopyTo（<c>collection_e2e</c> 非 Skip）。
/// IList 包装 ctor、GetEnumerator、受保护 Items、可重写 InsertItem 等仍后置——禁止静默 stub。
/// </remarks>
public class Collection<T> {
    private List<T> _items;

    /// <summary>以空列表初始化。</summary>
    public Collection() {
        _items = new List<T>();
    }

    /// <summary>包装既有列表（共享底层存储）。</summary>
    public Collection(List<T> list) {
        _items = list;
    }

    // ── 公共方法 ──

    /// <summary>添加元素。</summary>
    public void Add(T item) {
        _items.Add(item);
    }

    /// <summary>清空。</summary>
    public void Clear() {
        _items.Clear();
    }

    /// <summary>判断是否包含。</summary>
    public bool Contains(T item) {
        return _items.Contains(item);
    }

    /// <summary>移除元素。</summary>
    public bool Remove(T item) {
        return _items.Remove(item);
    }

    /// <summary>元素总数。</summary>
    public int Count {
        get { return _items.Count; }
    }

    // ── 索引器（C# this[int]；RFC 007）──

    /// <summary>索引器：<c>collection[i]</c> / <c>collection[i]=v</c>。</summary>
    public T this[int index] {
        get { return _items[index]; }
        set { _items[index] = value; }
    }

    /// <summary>查找下标。</summary>
    public int IndexOf(T item) {
        return _items.IndexOf(item);
    }

    /// <summary>插入元素。</summary>
    public void Insert(int index, T item) {
        _items.Insert(index, item);
    }

    /// <summary>移除指定下标元素。</summary>
    public void RemoveAt(int index) {
        _items.RemoveAt(index);
    }

    /// <summary>复制全部元素到目标数组（自 <paramref name="arrayIndex"/> 起）。</summary>
    public void CopyTo(T[] array, int arrayIndex)
    {
        _items.CopyTo(array, arrayIndex);
    }
}
