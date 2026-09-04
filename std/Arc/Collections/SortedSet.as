namespace Arc.Collections;

/// <summary>泛型排序集合——对齐 C# System.Collections.Generic.SortedSet&lt;T&gt;。</summary>
/// <typeparam name="T">元素类型。</typeparam>
/// <remarks>
/// 内部由红黑树实现，归 runtime ABI (<c>rt_sorted_set_*</c>) 管理。
/// 标量元素以指针位装箱（同 SortedDictionary / <c>rt_cmp_int</c>），非栈上 alloca 假指针。
/// 成熟度：Stable 最小面——ctor / Add / Contains / Remove / Min / Max / Count / Clear
/// （<c>sorted_set_e2e</c> 非 Skip）。比较器 ctor、Reverse、GetViewBetween、集合运算仍后置——禁止静默 stub。
/// 空集上 Min/Max 未定义（Stable 面勿依赖）。
/// </remarks>
public class SortedSet<T>
    where T : IComparable<T> {
    private int _handle;

    /// <summary>构造空排序集合（默认比较：标量按值；string 按内容）。</summary>
    public SortedSet() {
        _handle = 0;
    }

    // ── 核心操作 ──

    /// <summary>添加元素。已存在则返回 false。</summary>
    [Builtin(ABI = "rt_sorted_set_add")]
    public bool Add(T item) {
        return false;
    }

    /// <summary>判断是否包含指定元素。</summary>
    [Builtin(ABI = "rt_sorted_set_contains")]
    public bool Contains(T item) {
        return false;
    }

    /// <summary>移除指定元素。成功返回 true。</summary>
    [Builtin(ABI = "rt_sorted_set_remove")]
    public bool Remove(T item) {
        return false;
    }

    // ── 极值查询（非空集）──

    /// <summary>最小元素。空集未定义——Stable 面勿调用。</summary>
    [Builtin(ABI = "rt_sorted_set_min")]
    public T Min { get; }

    /// <summary>最大元素。空集未定义——Stable 面勿调用。</summary>
    [Builtin(ABI = "rt_sorted_set_max")]
    public T Max { get; }

    // ── 集合属性 ──

    /// <summary>元素总数。</summary>
    [Builtin(ABI = "rt_sorted_set_count")]
    public int Count { get; }

    /// <summary>清空所有元素。</summary>
    [Builtin(ABI = "rt_sorted_set_clear")]
    public void Clear() { }
}
