namespace Arc.Collections;

/// <summary>
/// 集合变更动作类型——对齐 RFC 037 §5.3 项级语义（Add/Remove/Update/Insert/Move/Clear）。
/// 对应 WPF NotifyCollectionChangedAction 的六动作面。
/// </summary>
public enum CollectionChangeAction {
    /// <summary>追加元素（Add 尾部）。</summary>
    Add,
    /// <summary>移除元素（Remove / RemoveAt）。</summary>
    Remove,
    /// <summary>替换元素（索引器 set）。</summary>
    Update,
    /// <summary>在下标处插入元素（Insert）。</summary>
    Insert,
    /// <summary>移动元素（Move）。</summary>
    Move,
    /// <summary>清空全部（Clear）。</summary>
    Clear,
}
