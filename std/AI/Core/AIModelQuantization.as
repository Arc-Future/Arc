// AIModelQuantization — 模型量化档位（RFC 041 §7.2 注册元数据）。
//
// 仅承载量化档位标签（预算/驱动决策用），不感知具体量化算法。Float32 为
// 未量化基线；Int4/Int8 为低内存档位（16G 普通电脑内存预算默认 4–6 GiB）。
namespace Arc.AI;

/// <summary>模型量化档位（RFC 041 §7.2）。</summary>
public enum AIModelQuantization {
    /// <summary>单精度浮点（未量化基线）。</summary>
    Float32,

    /// <summary>半精度浮点。</summary>
    Float16,

    /// <summary>8 位整数量化。</summary>
    Int8,

    /// <summary>4 位整数量化（低内存档）。</summary>
    Int4,
}
