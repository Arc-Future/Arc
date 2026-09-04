namespace Arc.Collections;

/// <summary>
/// 集合变更通知事件参数——最小面：kind/index/oldIndex/item（对齐 RFC 037 §5.3
/// 「变更表面 CollectionChanged（kind / index / item）」）。
/// </summary>
/// <typeparam name="T">元素类型。</typeparam>
/// <remarks>
/// 语义（与 WPF NotifyCollectionChangedEventArgs 对应字段对齐）：
///   - Add/Insert：<see cref="Index"/> = 新项下标，<see cref="NewItem"/> = 新项；
///   - Remove/RemoveAt：<see cref="Index"/> = 被移除项原下标，<see cref="OldItem"/> = 被移除项；
///   - Update（索引器 set）：<see cref="Index"/> = 变更下标，<see cref="OldItem"/> = 旧值，
///     <see cref="NewItem"/> = 新值；
///   - Move：<see cref="OldIndex"/> = 原位置，<see cref="Index"/> = 新位置，
///     <see cref="NewItem"/> / <see cref="OldItem"/> = 被移动项；
///   - Clear：<see cref="Index"/> / <see cref="OldIndex"/> = -1，新旧项为 default。
/// 不适用的字段为 default(T)；struct 值承载（零装箱，对齐 KeyValuePair&lt;K,V&gt; 先例）。
/// </remarks>
public struct CollectionChangedEventArgs<T> {
    /// <summary>变更动作（kind）。</summary>
    public CollectionChangeAction Action;
    /// <summary>变更所在下标（Clear = -1）。</summary>
    public int Index;
    /// <summary>移动的源位置；其余动作 = -1。</summary>
    public int OldIndex;
    /// <summary>新项（Add/Insert/Update/Move）；其余 default(T)。</summary>
    public T NewItem;
    /// <summary>旧项（Remove/Update/Move）；其余 default(T)。</summary>
    public T OldItem;
}
