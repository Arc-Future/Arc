namespace Arc.Collections;

/// <summary>动态列表——对齐 C# System.Collections.Generic.List&lt;T&gt;。</summary>
/// <typeparam name="T">元素类型。</typeparam>
/// <remarks>
/// 实现由 runtime ABI (rt_list_*) 提供。codegen 按类名模式拦截：
/// 值类型 <c>Add</c> / 索引器直访 <c>RtList</c>（冷路径 <c>rt_list_ensure_capacity</c>；
/// 无热路径 <c>rt_list_push</c>/<c>rt_list_get</c>）；引用元素仍走 <c>rt_list_*</c> 维护 ARC。
/// 谓词/排序方法通过 rt_list_find_get 等 ABI 实现。
/// C# 索引器：`list[i]` / `list[i]=v` → MIR `get_Item`/`set_Item` →
/// codegen 直访 RtList.data（bounds + GEP + load/store）。
/// </remarks>
public class List<T> : IEnumerable<T> {
    private int _handle;

    // ── 构造函数 ──

    /// <summary>构造空列表。</summary>
    public List() {
        _handle = 0;
    }

    /// <summary>构造指定容量的列表。</summary>
    public List(int capacity) {
        _handle = 0;
    }

    // ── 索引器（C# this[int]；RFC 007，单一惯用）──

    /// <summary>索引器：读取/写入指定下标的元素（<c>list[i]</c>）。</summary>
    /// <remarks>codegen 直访 RtList.data（bounds + GEP + load/store）。</remarks>
    public T this[int index] {
        get { return 0; }
        set { }
    }

    // ── 集合属性 ──

    /// <summary>元素总数。</summary>
    [Builtin(ABI = "rt_list_size")]
    public int Count { get; }

    /// <summary>是否为只读列表。始终返回 false。</summary>
    [Builtin(ABI = "rt_list_is_read_only")]
    public bool IsReadOnly { get; }

    /// <summary>容量。获取底层缓冲容量；设置预留容量（下限截断到 Count，不缩减已分配空间时不收缩）。</summary>
    [Builtin(ABI = "rt_list_capacity")]
    public int Capacity { get; set; }

    // ── 核心 CRUD ──

    [Builtin(ABI = "rt_list_push")]
    public void Add(T item) { }

    [Builtin(ABI = "rt_list_contains")]
    public bool Contains(T item) {
        return false;
    }

    [Builtin(ABI = "rt_list_index_of")]
    public int IndexOf(T item) {
        return -1;
    }

    /// <summary>从后向前查找元素下标；未找到返回 -1。</summary>
    [Builtin(ABI = "rt_list_last_index_of")]
    public int LastIndexOf(T item) {
        return -1;
    }

    [Builtin(ABI = "rt_list_insert")]
    public void Insert(int index, T item) { }

    [Builtin(ABI = "rt_list_remove_at")]
    public void RemoveAt(int index) { }

    [Builtin(ABI = "rt_list_remove")]
    public bool Remove(T item) {
        return false;
    }

    [Builtin(ABI = "rt_list_clear")]
    public void Clear() { }

    [Builtin(ABI = "rt_list_reverse")]
    public void Reverse() { }

    // ── 谓词操作 ──

    [Builtin(ABI = "rt_list_find_get")]
    public T Find(Func<T, bool> predicate) {
        return 0;
    }

    [Builtin(ABI = "rt_list_find_all")]
    public List<T> FindAll(Func<T, bool> predicate) {
        return 0;
    }

    [Builtin(ABI = "rt_list_exists")]
    public bool Exists(Func<T, bool> predicate) {
        return false;
    }

    /// <summary>返回第一个满足谓词的下标；未找到返回 -1。</summary>
    [Builtin(ABI = "rt_list_find_index")]
    public int FindIndex(Func<T, bool> predicate) {
        return -1;
    }

    /// <summary>返回最后一个满足谓词的下标；未找到返回 -1。</summary>
    [Builtin(ABI = "rt_list_find_last_index")]
    public int FindLastIndex(Func<T, bool> predicate) {
        return -1;
    }

    /// <summary>是否全部元素满足谓词（空列表为 true）。</summary>
    [Builtin(ABI = "rt_list_true_for_all")]
    public bool TrueForAll(Func<T, bool> predicate) {
        return false;
    }

    [Builtin(ABI = "rt_list_for_each")]
    public void ForEach(Action<T> action) { }

    [Builtin(ABI = "rt_list_remove_all")]
    public int RemoveAll(Func<T, bool> predicate) {
        return 0;
    }

    // ── 排序 ──

    [Builtin(ABI = "rt_list_sort_default")]
    public void Sort() { }

    [Builtin(ABI = "rt_list_sort")]
    public void Sort(Func<T, T, int> cmp) { }

    // ── 数组转换 / Span 视图（RFC 005 M2）──

    /// <summary>零拷贝连续视图；扩容（Add）后持有的 Span 失效。</summary>
    public Span<T> AsSpan() {
        return 0;
    }

    /// <summary>子区间零拷贝视图。</summary>
    public Span<T> AsSpan(int start, int length) {
        return 0;
    }

    /// <summary>只读零拷贝连续视图。</summary>
    public ReadOnlySpan<T> AsReadOnlySpan() {
        return 0;
    }

    [Builtin(ABI = "rt_list_to_array")]
    public T[] ToArray() {
        return 0;
    }

    [Builtin(ABI = "rt_list_copy_to")]
    public void CopyTo(T[] array, int start) { }

    // ── 批量操作 ──

    [Builtin(ABI = "rt_list_add_range_list")]
    public void AddRange(IEnumerable<T> items) { }

    [Builtin(ABI = "rt_list_insert_range")]
    public void InsertRange(int index, IEnumerable<T> items) { }

    [Builtin(ABI = "rt_list_remove_range")]
    public void RemoveRange(int index, int count) { }

    [Builtin(ABI = "rt_list_get_range")]
    public List<T> GetRange(int index, int count) {
        return 0;
    }

    // ── 二分查找 ──

    [Builtin(ABI = "rt_list_binary_search")]
    public int BinarySearch(T item) {
        return -1;
    }

    [Builtin(ABI = "rt_list_binary_search_cmp")]
    public int BinarySearch(T item, IComparer<T> comparer) {
        return -1;
    }

    // ── 修剪 ──

    [Builtin(ABI = "rt_list_trim_excess")]
    public void TrimExcess() { }

    // ── 枚举 ──

    [Builtin(ABI = "rt_list_get_enumerator")]
    public IEnumerator<T> GetEnumerator() {
        return 0;
    }
}
