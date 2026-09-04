namespace Arc.Collections;

/// <summary>集合操作接口——对齐 C# System.Collections.Generic.ISet&lt;T&gt;。</summary>
/// <typeparam name="T">元素类型。</typeparam>
public interface ISet<T> : ICollection<T> {
    /// <summary>添加元素。已存在则返回 false。</summary>
    bool Add(T item);

    // ── 集合运算（原地修改）──

    /// <summary>并集：添加 other 中所有元素。</summary>
    void UnionWith(IEnumerable<T> other);

    /// <summary>交集：仅保留同时存在于 other 的元素。</summary>
    void IntersectWith(IEnumerable<T> other);

    /// <summary>差集：移除 other 中也存在的元素。</summary>
    void ExceptWith(IEnumerable<T> other);

    /// <summary>对称差：保留仅存在于一侧的元素。</summary>
    void SymmetricExceptWith(IEnumerable<T> other);

    // ── 集合判定（只读）──

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
