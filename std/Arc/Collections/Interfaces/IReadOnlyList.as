namespace Arc.Collections;

/// <summary>只读可索引集合接口——对齐 C# System.Collections.Generic.IReadOnlyList&lt;T&gt;。</summary>
/// <typeparam name="T">元素类型。</typeparam>
/// <remarks>
/// 索引访问唯一惯用：<c>this[int]</c>（编译为 <c>get_Item</c>）。
/// </remarks>
public interface IReadOnlyList<out T> : IReadOnlyCollection<T> {
    /// <summary>索引器 get：读取指定下标的元素（<c>list[i]</c>）。</summary>
    T this[int index] { get; }
}
