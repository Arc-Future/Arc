namespace Arc.Collections;

/// <summary>默认比较器——对齐 C# System.Collections.Generic.Comparer&lt;T&gt;。</summary>
/// <typeparam name="T">待比较的类型。</typeparam>
public class Comparer<T> : IComparer<T>
    where T : IComparable<T> {
    /// <summary>默认实例（static readonly 惰性单例）。</summary>
    public static readonly Comparer<T> Default = new Comparer<T>();

    /// <summary>比较两个值。返回 &lt;0（x&lt;y）、0（相等）、&gt;0（x&gt;y）。</summary>
    public int Compare(T x, T y) {
        return x.CompareTo(y);
    }
}
