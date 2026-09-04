namespace Arc.Collections;

/// <summary>可索引集合接口——对齐 C# System.Collections.Generic.IList&lt;T&gt;。</summary>
/// <typeparam name="T">元素类型。</typeparam>
/// <remarks>
/// 索引访问唯一惯用：<c>this[int]</c>（编译为 <c>get_Item</c>/<c>set_Item</c>）。
/// </remarks>
public interface IList<T> : ICollection<T> {
    /// <summary>索引器：读取/写入指定下标的元素（<c>list[i]</c>）。</summary>
    T this[int index] { get; set; }

    /// <summary>返回指定元素首次出现的下标。不存在返回 -1。</summary>
    int IndexOf(T item);

    /// <summary>在指定下标插入元素。</summary>
    void Insert(int index, T item);

    /// <summary>移除指定下标处的元素。</summary>
    void RemoveAt(int index);
}
