namespace Arc.Collections;

/// <summary>相等比较器接口——对齐 C# System.Collections.Generic.IEqualityComparer&lt;T&gt;。</summary>
/// <typeparam name="T">待比较的类型。</typeparam>
public interface IEqualityComparer<in T> {
    /// <summary>判断两个值是否相等。</summary>
    bool Equals(T x, T y);

    /// <summary>获取哈希码。</summary>
    int GetHashCode(T obj);
}
