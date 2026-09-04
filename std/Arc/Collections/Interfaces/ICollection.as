namespace Arc.Collections;

/// <summary>泛型集合基础接口——对齐 C# System.Collections.Generic.ICollection&lt;T&gt;。</summary>
/// <typeparam name="T">元素类型。</typeparam>
public interface ICollection<T> : IEnumerable<T> {
    /// <summary>元素总数。</summary>
    int Count { get; }

    /// <summary>是否为只读集合。</summary>
    bool IsReadOnly { get; }

    /// <summary>添加元素。</summary>
    void Add(T item);

    /// <summary>清空所有元素。</summary>
    void Clear();

    /// <summary>判断是否包含指定元素。</summary>
    bool Contains(T item);

    /// <summary>拷贝到目标数组。</summary>
    void CopyTo(T[] array, int arrayIndex);

    /// <summary>移除指定元素。成功返回 true。</summary>
    bool Remove(T item);
}
