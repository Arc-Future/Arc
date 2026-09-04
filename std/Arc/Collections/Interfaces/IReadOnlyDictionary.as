namespace Arc.Collections;

/// <summary>只读键值对集合接口——对齐 C# System.Collections.Generic.IReadOnlyDictionary&lt;K, V&gt;。</summary>
/// <typeparam name="K">键类型。</typeparam>
/// <typeparam name="V">值类型。</typeparam>
/// <remarks>
/// 索引访问唯一惯用：<c>this[K]</c>（编译为 <c>get_Item</c>）。
/// </remarks>
public interface IReadOnlyDictionary<K, V> : IReadOnlyCollection<KeyValuePair<K, V>> {
    /// <summary>索引器 get：读与指定键关联的值（<c>dict[k]</c>）。</summary>
    V this[K key] { get; }

    /// <summary>所有键。</summary>
    K[] Keys { get; }

    /// <summary>所有值。</summary>
    V[] Values { get; }

    /// <summary>判断是否包含指定键。</summary>
    bool ContainsKey(K key);

    /// <summary>尝试获取值。存在返回 true。</summary>
    bool TryGetValue(K key, out V value);
}
