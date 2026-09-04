namespace Arc.Collections;

/// 枚举器接口——支持对集合的当前元素与前进操作（对齐 C# System.Collections.Generic.IEnumerator<T>）。
public interface IEnumerator<out T> {
    /// <summary>前进到下一个元素。</summary>
    /// <returns>成功前进返回 true；越过集合末尾返回 false。</returns>
    bool MoveNext();

    /// <summary>当前指向的元素。</summary>
    T Current { get; }
}
