namespace Arc.Collections;

/// <summary>默认相等比较器——对齐 C# System.Collections.Generic.EqualityComparer&lt;T&gt;。</summary>
/// <typeparam name="T">待比较的类型。</typeparam>
public class EqualityComparer<T> : IEqualityComparer<T>
    where T : IEquatable<T>, IHashable<T> {
    /// <summary>默认实例（static readonly 惰性单例：无状态分派器，首触构造一次、线程安全）。</summary>
    public static readonly EqualityComparer<T> Default = new EqualityComparer<T>();

    /// <summary>判断两个值是否相等。通过 IEquatable&lt;T&gt; 的静态方法分派，零装箱。</summary>
    public bool Equals(T x, T y) {
        return T.Equals(x, y);
    }

    /// <summary>获取哈希码。通过 IHashable&lt;T&gt; 的静态方法分派，零装箱。</summary>
    public int GetHashCode(T obj) {
        return T.GetHashCode(obj);
    }
}
