namespace Arc.Collections;

/// <summary>泛型字典——对齐 C# System.Collections.Generic.Dictionary&lt;K, V&gt;。</summary>
/// <typeparam name="K">键类型。</typeparam>
/// <typeparam name="V">值类型。</typeparam>
/// <remarks>
/// 核心 hash 查找由 runtime ABI (rt_dict_*) 提供。
/// C# 索引器：<c>dict[k]</c> / <c>dict[k]=v</c> → MIR <c>get_Item</c>/<c>set_Item</c>
/// → codegen 内联 <c>rt_dict_get</c>/<c>rt_dict_set</c>。
/// <c>TryGetValue</c>：源码体为 Builtin 占位；真路径 = codegen <c>rt_dict_try_get_value</c> + out 槽
/// （禁静默 false；证据 <c>h1_core_regression_e2e</c> / UnitTest）。未走 builtin 的链接 stub → <c>rt_panic</c>。
/// </remarks>
public class Dictionary<K, V>
    where K : IEquatable<K> {
    private int _handle;

    // ── 构造函数 ──

    /// <summary>构造空字典。</summary>
    public Dictionary() {
        _handle = 0;
    }

    /// <summary>构造指定初始容量的字典。</summary>
    public Dictionary(int capacity) {
        _handle = 0;
    }

    // ── 索引器（C# this[K]；RFC 007，单一惯用）──

    /// <summary>索引器：读取/写入键对应的值（<c>dict[k]</c>）。键不存在时 get 返回 default(V)。</summary>
    public V this[K key] {
        get { return 0; }
        set { }
    }

    // ── 核心操作 ──

    /// <summary>尝试添加键值对。键已存在返回 false，不覆盖。</summary>
    /// <remarks>单次哈希查找，零 ContainsKey+Set 双重查找开销。</remarks>
    [Builtin(ABI = "rt_dict_try_add")]
    public bool Add(K key, V value) {
        return false;
    }

    /// <summary>尝试获取值。存在返回 true 并输出值；不存在 value = default(V)。</summary>
    /// <remarks>单次哈希查找，零 ContainsKey+Get 双重查找开销。</remarks>
    [Builtin(ABI = "rt_dict_try_get_value")]
    public bool TryGetValue(K key, out V value) {
        return false;
    }

    /// <summary>移除指定键。成功返回 true。</summary>
    [Builtin(ABI = "rt_dict_remove")]
    public bool Remove(K key) {
        return false;
    }

    /// <summary>判断是否包含指定键。</summary>
    [Builtin(ABI = "rt_dict_contains")]
    public bool ContainsKey(K key) {
        return false;
    }

    /// <summary>判断是否包含指定值（按值相等；标量/string 走 runtime eq）。</summary>
    [Builtin(ABI = "rt_dict_contains_value")]
    public bool ContainsValue(V value) {
        return false;
    }

    // ── 集合属性 ──

    /// <summary>元素总数。</summary>
    [Builtin(ABI = "rt_dict_count")]
    public int Count { get; }

    /// <summary>清空所有元素。</summary>
    [Builtin(ABI = "rt_dict_clear")]
    public void Clear() { }

    // ── 键/值集合 ──

    /// <summary>所有键的数组快照。</summary>
    [Builtin(ABI = "rt_dict_keys")]
    public K[] Keys { get; }

    /// <summary>所有值的数组快照。</summary>
    [Builtin(ABI = "rt_dict_values")]
    public V[] Values { get; }

    // ── 枚举 ──

    /// <summary>返回键值对枚举器。</summary>
    [Builtin(ABI = "rt_dict_get_enumerator")]
    public IEnumerator<KeyValuePair<K, V>> GetEnumerator() {
        return 0;
    }
}
