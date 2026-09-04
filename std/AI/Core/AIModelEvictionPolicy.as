// AIModelEvictionPolicy — 内存预算驱逐策略（RFC 041 §7.2）。
//
// None = 不驱逐（预算超限直接拒绝加载）；Lru = 超限时按最近最少使用驱逐空闲
// （引用计数归零）模型腾挪预算。预算计账由 AIModelBudget 承载，策略裁决由
// AIModelPolicy 承载。
namespace Arc.AI;

/// <summary>内存预算驱逐策略（RFC 041 §7.2）。</summary>
public enum AIModelEvictionPolicy {
    /// <summary>不驱逐。预算超限拒绝加载（AIModelBudgetExceededException）。</summary>
    None,

    /// <summary>LRU 驱逐：超限时驱逐空闲（refcount==0）中最久未用的模型。</summary>
    Lru,
}
