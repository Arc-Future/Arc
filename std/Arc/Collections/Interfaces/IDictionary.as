namespace Arc.Collections;

/// <summary>键值对集合接口——对齐 C# System.Collections.Generic.IDictionary&lt;K, V&gt;。</summary>
/// <typeparam name="K">键类型。</typeparam>
/// <typeparam name="V">值类型。</typeparam>
/// <remarks>
/// 索引访问唯一惯用：<c>this[K]</c>（编译为 <c>get_Item</c>/<c>set_Item</c>）。
/// </remarks>
public interface IDictionary<K, V> : ICollection<KeyValuePair<K, V>> {
    /// <summary>索引器：读/写与指定键关联的值（<c>dict[k]</c>）。</summary>
    V this[K key] { get; set; }

    /// <summary>所有键。</summary>
    K[] Keys { get; }

    /// <summary>所有值。</summary>
    V[] Values { get; }

    /// <summary>判断是否包含指定键。</summary>
    bool ContainsKey(K key);

    /// <summary>添加键值对。键已存在则返回 false。</summary>
    bool Add(K key, V value);

    /// <summary>移除指定键。成功返回 true。</summary>
    bool Remove(K key);

    /// <summary>尝试获取值。存在返回 true。</summary>
    bool TryGetValue(K key, out V value);
}
