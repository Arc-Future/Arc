// AIModelLoadPolicy — 模型加载策略（RFC 041 §7.2 注册元数据）。
//
// Lazy = 首次 Acquire 才加载（默认，创建成本一次性摊销）；Eager = 注册时即加载；
// Warm = 注册时预热常驻。加载时机由注册表依策略执行。
namespace Arc.AI;

/// <summary>模型加载策略（RFC 041 §7.2）。</summary>
public enum AIModelLoadPolicy {
    /// <summary>懒加载：首次 Acquire 命中才创建底层 runner。</summary>
    Lazy,

    /// <summary>注册时即加载。</summary>
    Eager,

    /// <summary>注册时预热并常驻。</summary>
    Warm,
}
