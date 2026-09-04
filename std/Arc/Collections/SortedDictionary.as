namespace Arc.Collections;

/// <summary>泛型排序字典——对齐 C# System.Collections.Generic.SortedDictionary&lt;K, V&gt;。</summary>
/// <typeparam name="K">键类型。</typeparam>
/// <typeparam name="V">值类型。</typeparam>
/// <remarks>
/// 内部由红黑树实现，按键排序，归 runtime ABI (<c>rt_sorted_dict_*</c>) 管理。
/// 标量键/值以指针位装箱（<c>inttoptr</c> / <c>rt_cmp_int</c>），非栈上 alloca 假指针。
/// 成熟度：Stable 最小面——ctor / 索引器 / Add / TryGetValue / ContainsKey / Remove / Count / Clear
/// （<c>sorted_dictionary_e2e</c> 非 Skip）。比较器 ctor、Keys/Values 仍后置——禁止静默 stub。
/// </remarks>
public class SortedDictionary<K, V>
    where K : IComparable<K> {
    private int _handle;

    /// <summary>构造空排序字典（默认比较：标量按值；string 按内容）。</summary>
    public SortedDictionary() {
        _handle = 0;
    }

    // ── 索引器（C# this[K]；RFC 007）──
    // get/set 由 codegen 按类名 + get_Item/set_Item 识别（与 Dictionary 一致）。

    /// <summary>索引器：读取/写入键对应的值（<c>dict[k]</c>）。缺键 get 行为未定义——Stable 面先 ContainsKey/TryGetValue。</summary>
    public V this[K key] {
        get { return 0; }
        set { }
    }

    // ── 核心操作 ──

    /// <summary>添加键值对。键已存在则返回 false。</summary>
    [Builtin(ABI = "rt_sorted_dict_add")]
    public bool Add(K key, V value) {
        return false;
    }

    /// <summary>尝试获取值。存在返回 true。</summary>
    [Builtin(ABI = "rt_sorted_dict_try_get")]
    public bool TryGetValue(K key, out V value) {
        value = default(V);
        return false;
    }

    /// <summary>移除指定键。成功返回 true。</summary>
    [Builtin(ABI = "rt_sorted_dict_remove")]
    public bool Remove(K key) {
        return false;
    }

    /// <summary>判断是否包含指定键。</summary>
    [Builtin(ABI = "rt_sorted_dict_contains")]
    public bool ContainsKey(K key) {
        return false;
    }

    // ── 集合属性 ──

    /// <summary>元素总数。</summary>
    [Builtin(ABI = "rt_sorted_dict_count")]
    public int Count { get; }

    /// <summary>清空所有元素。</summary>
    [Builtin(ABI = "rt_sorted_dict_clear")]
    public void Clear() { }
}
