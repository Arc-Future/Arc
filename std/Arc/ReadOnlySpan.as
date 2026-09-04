namespace Arc;

// ReadOnlySpan<T> —— 只读连续切片视图（RFC 005）。
//
// 语言内建 ref-like 值类型（TypeId::Span { mutable: false }）：索引只读；
// 可由 Span 隐式转换而来。本文件为契约 / 文档面（builtin facade，跳过方法体）。
//
// 已接线（非 Skip e2e / UnitTest）：
//   Length · this[i] 只读 · Slice(start) / Slice(start,length) · IsEmpty · Empty
//   CopyTo(Span) · TryCopyTo(Span) · ToArray()
//   foreach（MIR 索引脱糖：Length + IndexGet；零堆枚举器对象）
// 构造：T[].AsReadOnlySpan / string.AsSpan→ReadOnlySpan&lt;byte&gt; / […]→ROS / Span 隐式转换
//
// 后置（诚实；禁止 NotImplemented 假 Stable）：
//   显式 GetEnumerator / IEnumerator 协议 · 内容相等
//   UTF-16 码元语义（string.AsSpan 为 UTF-8 码元，非 C# char）

/// <summary>只读连续切片视图（零拷贝；寿命=借用）。</summary>
public class ReadOnlySpan {
    /// <summary>元素个数。</summary>
    public int Length { get; }

    /// <summary>是否为空（Length == 0）。</summary>
    public bool IsEmpty { get; }

    /// <summary>空视图（等价于 <c>ReadOnlySpan&lt;T&gt; r = [];</c>）。用法：<c>ReadOnlySpan&lt;int&gt;.Empty</c>。</summary>
    public static ReadOnlySpan Empty { get; }

    /// <summary>按索引只读元素；越界 panic；写入编译错误。</summary>
    public int this[int index] { get; }

    /// <summary>子切片视图（零拷贝）。</summary>
    public ReadOnlySpan Slice(int start) { }

    public ReadOnlySpan Slice(int start, int length) { }

    /// <summary>将本视图元素复制到可变目标 Span；目标过短则 panic。</summary>
    public void CopyTo(Span destination) { }

    /// <summary>尝试复制到目标 Span；过短返回 false（不 panic），成功返回 true。</summary>
    public bool TryCopyTo(Span destination) { }

    /// <summary>分配新 <c>T[]</c> 并拷贝本视图元素（与底层缓冲解耦）。</summary>
    public int[] ToArray() { }
}
