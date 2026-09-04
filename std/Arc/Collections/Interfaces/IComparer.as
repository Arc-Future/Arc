namespace Arc.Collections;

/// <summary>比较器接口——对齐 C# System.Collections.Generic.IComparer&lt;T&gt;。</summary>
/// <typeparam name="T">待比较的类型。</typeparam>
public interface IComparer<in T> {
    /// <summary>比较两个值。返回 &lt;0（x&lt;y）、0（相等）、&gt;0（x&gt;y）。</summary>
    int Compare(T x, T y);
}
