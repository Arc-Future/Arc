namespace Arc;

// Span<T> —— 可变连续切片视图（RFC 005）。
//
// 语言内建 ref-like 值类型（TypeId::Span { mutable: true }）：方法体由编译器 Builtin
// 实现，禁止用户手写指针。本文件为契约 / 文档面（builtin facade，跳过方法体）。
//
// 已接线（非 Skip e2e / UnitTest）：
//   Length · this[i] 读写 · Slice(start) / Slice(start,length) · Fill/Clear · AsReadOnly
//   IsEmpty · Empty · CopyTo(Span) · TryCopyTo(Span) · ToArray()
//   foreach（MIR 索引脱糖：Length + IndexGet；零堆枚举器对象）
// 构造：T[].AsSpan / List.AsSpan / […]→Span 栈缓冲（见 RFC 005 M1/M2/M2b）
//
// 后置（诚实；禁止 NotImplemented 假 Stable）：
//   显式 GetEnumerator / IEnumerator 协议 · 内容相等（SequenceEqual）
//   arr[start..end] Range 语法 · DangerousGetPinnableReference 永拒

/// <summary>可变连续切片视图（零拷贝；寿命=借用）。</summary>
public class Span {
    /// <summary>元素个数。</summary>
    public int Length { get; }

    /// <summary>是否为空（Length == 0）。</summary>
    public bool IsEmpty { get; }

    /// <summary>空视图（等价于 <c>Span&lt;T&gt; s = [];</c>）。用法：<c>Span&lt;int&gt;.Empty</c>。</summary>
    public static Span Empty { get; }

    /// <summary>按索引读写元素；越界 panic（与 T[] 同策略）。</summary>
    public int this[int index] { get; set; }

    /// <summary>子切片视图（零拷贝）。</summary>
    public Span Slice(int start) { }

    public Span Slice(int start, int length) { }

    public void Fill(int value) { }

    public void Clear() { }

    /// <summary>只读视图（同一缓冲；类型层降为 ReadOnlySpan）。</summary>
    public ReadOnlySpan AsReadOnly() { }

    /// <summary>将本视图元素复制到目标 Span；目标过短则 panic。</summary>
    public void CopyTo(Span destination) { }

    /// <summary>尝试复制到目标 Span；过短返回 false（不 panic），成功返回 true。</summary>
    public bool TryCopyTo(Span destination) { }

    /// <summary>分配新 <c>T[]</c> 并拷贝本视图元素（与底层缓冲解耦）。</summary>
    public int[] ToArray() { }
}
