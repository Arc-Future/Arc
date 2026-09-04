namespace Arc.Collections;

/// <summary>只读集合接口——对齐 C# System.Collections.Generic.IReadOnlySet&lt;T&gt;。</summary>
/// <typeparam name="T">元素类型。</typeparam>
public interface IReadOnlySet<T> : IReadOnlyCollection<T> {
    /// <summary>判断是否包含指定元素。</summary>
    bool Contains(T item);

    /// <summary>是否为 other 的子集。</summary>
    bool IsSubsetOf(IEnumerable<T> other);

    /// <summary>是否为 other 的超集。</summary>
    bool IsSupersetOf(IEnumerable<T> other);

    /// <summary>是否为 other 的真子集。</summary>
    bool IsProperSubsetOf(IEnumerable<T> other);

    /// <summary>是否为 other 的真超集。</summary>
    bool IsProperSupersetOf(IEnumerable<T> other);

    /// <summary>是否与 other 有交集。</summary>
    bool Overlaps(IEnumerable<T> other);

    /// <summary>是否与 other 元素完全相同。</summary>
    bool SetEquals(IEnumerable<T> other);
}
