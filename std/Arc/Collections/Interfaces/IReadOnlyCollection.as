namespace Arc.Collections;

/// <summary>只读集合接口——对齐 C# System.Collections.Generic.IReadOnlyCollection&lt;T&gt;。</summary>
/// <typeparam name="T">元素类型。</typeparam>
public interface IReadOnlyCollection<out T> : IEnumerable<T> {
    /// <summary>元素总数。</summary>
    int Count { get; }
}
