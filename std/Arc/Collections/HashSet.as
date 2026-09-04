namespace Arc.Collections;

/// <summary>泛型哈希集合——对齐 C# System.Collections.Generic.HashSet&lt;T&gt;。</summary>
/// <typeparam name="T">元素类型。</typeparam>
/// <remarks>
/// Stable：CRUD + 原地集合运算 / 判定由 <c>rt_set_*</c> 提供（UnitTest <c>HashSetTests</c> +
/// <c>std_collections_e2e</c> 非 Skip，含 <c>HashSet&lt;string&gt;</c>）。集合运算实参为另一 <c>HashSet&lt;T&gt;</c>
///（runtime 仅 set↔set；通用 <c>IEnumerable</c> 实参仍后置）。定制 <c>IEqualityComparer</c> ctor 仍后置——禁止静默 stub。
/// </remarks>
public class HashSet<T>
    where T : IEquatable<T> {
    private int _handle;

    // ── 构造函数 ──

    public HashSet() {
        _handle = 0;
    }

    public HashSet(int capacity) {
        _handle = 0;
    }

    // ── 核心操作 ──

    [Builtin(ABI = "rt_set_add")]
    public bool Add(T item) {
        return false;
    }

    [Builtin(ABI = "rt_set_contains")]
    public bool Contains(T item) {
        return false;
    }

    [Builtin(ABI = "rt_set_remove")]
    public bool Remove(T item) {
        return false;
    }

    // ── 集合运算（原地修改；other 须为 HashSet）──

    [Builtin(ABI = "rt_set_union_with")]
    public void UnionWith(HashSet<T> other) { }

    [Builtin(ABI = "rt_set_intersect_with")]
    public void IntersectWith(HashSet<T> other) { }

    [Builtin(ABI = "rt_set_except_with")]
    public void ExceptWith(HashSet<T> other) { }

    [Builtin(ABI = "rt_set_symmetric_except_with")]
    public void SymmetricExceptWith(HashSet<T> other) { }

    // ── 集合判定（只读；other 须为 HashSet）──

    [Builtin(ABI = "rt_set_is_subset_of")]
    public bool IsSubsetOf(HashSet<T> other) {
        return false;
    }

    [Builtin(ABI = "rt_set_is_superset_of")]
    public bool IsSupersetOf(HashSet<T> other) {
        return false;
    }

    [Builtin(ABI = "rt_set_is_proper_subset_of")]
    public bool IsProperSubsetOf(HashSet<T> other) {
        return false;
    }

    [Builtin(ABI = "rt_set_is_proper_superset_of")]
    public bool IsProperSupersetOf(HashSet<T> other) {
        return false;
    }

    [Builtin(ABI = "rt_set_overlaps")]
    public bool Overlaps(HashSet<T> other) {
        return false;
    }

    [Builtin(ABI = "rt_set_set_equals")]
    public bool SetEquals(HashSet<T> other) {
        return false;
    }

    // ── 集合属性 ──

    [Builtin(ABI = "rt_set_count")]
    public int Count { get; }

    [Builtin(ABI = "rt_set_clear")]
    public void Clear() { }

    [Builtin(ABI = "rt_set_to_array")]
    public T[] ToArray() {
        return 0;
    }

    // ── 枚举 ──

    [Builtin(ABI = "rt_set_get_enumerator")]
    public IEnumerator<T> GetEnumerator() {
        return 0;
    }
}
