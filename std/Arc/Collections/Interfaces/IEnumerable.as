namespace Arc.Collections;

/// 可枚举接口——支持简单迭代（对标 C# System.Collections.Generic.IEnumerable<T>）。
public interface IEnumerable<out T> {
    /// <summary>返回枚举器，用于遍历集合元素。</summary>
    /// <returns>集合的枚举器。</returns>
    IEnumerator<T> GetEnumerator();
}
