namespace Arc.Linq;

using Arc.Collections;

/// <summary>
/// <c>group … by …</c> 查询子句的分组产物——对标 C# <c>System.Linq.IGrouping&lt;TKey, TElement&gt;</c>。
///
/// <b>诚实子集（Stable，MIR 物化专用）</b>：
/// - 由编译器 <c>groupby</c> 物化路径构造（<c>new Grouping(key)</c>），Key 只读；
/// - <c>Items</c> / <c>Count</c> 暴露组内元素；<c>Add</c> 供物化填充；
/// - 分组等值判定走 key 的 <c>Compare == 0</c>（与 orderby 同支持面），
///   非 <c>Equals</c>/<c>GetHashCode</c> 语义（对象哈希后置）。
/// 用户代码不直接构造本类。
/// </summary>
/// <typeparam name="K">分组键类型。</typeparam>
/// <typeparam name="T">组内元素类型。</typeparam>
public class Grouping<K, T> {
    private K _key;
    private List<T> _items;

    /// <summary>构造分组并绑定键。</summary>
    public Grouping(K key) {
        _key = key;
        _items = new List<T>();
    }

    /// <summary>分组键（只读）。</summary>
    public K Key {
        get { return _key; }
    }

    /// <summary>组内元素（只读视图）。</summary>
    public List<T> Items {
        get { return _items; }
    }

    /// <summary>组内元素总数。</summary>
    public int Count {
        get { return _items.Count; }
    }

    /// <summary>追加元素到组内（物化填充用）。</summary>
    public void Add(T item) {
        _items.Add(item);
    }
}
