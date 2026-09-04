// AIModelRegistryOptions — 进程级模型注册表配置（RFC 041 §7.2）。
//
// 承载内存预算 / 驱逐策略 / 温窗 / 单模型并发上限。16G 普通电脑内存预算默认
// 4 GiB（RFC 041 §7.1 建议 4–6 GiB）；0 = 不设限。Eviction=None 时预算超限
// 直接拒绝加载；Lru 时先按最近最少使用驱逐空闲模型。
namespace Arc.AI;

/// <summary>
/// 模型注册表配置（RFC 041 §7.2）。经 <see cref="AIModelRegistry"/> 构造注入；
/// 修改需在构造前完成（注册表持有快照）。
/// </summary>
public class AIModelRegistryOptions {
    /// <summary>默认内存预算字节数（4 GiB，16G 普通电脑建议档）。</summary>
    private const long DefaultBudgetBytes = 0x100000000;

    /// <summary>常驻内存预算（按 SizeBytes 计账）；0 = 不设限。</summary>
    public long MemoryBudgetBytes;

    /// <summary>预算超限时的驱逐策略（None = 拒绝加载 / Lru = 驱逐空闲）。</summary>
    public AIModelEvictionPolicy Eviction;

    /// <summary>引用归零后的温窗保持秒数（0 = 立即卸载；&gt;0 = 温窗内保持常驻）。</summary>
    public int WarmKeepSeconds;

    /// <summary>单模型并发 Acquire 上限（默认 1，串行化；超限 Acquire 拒绝）。</summary>
    public int MaxConcurrentPerModel;

    public AIModelRegistryOptions() {
        this.MemoryBudgetBytes = AIModelRegistryOptions.DefaultBudgetBytes;
        this.Eviction = AIModelEvictionPolicy.Lru;
        this.WarmKeepSeconds = 30;
        this.MaxConcurrentPerModel = 1;
    }

    /// <summary>默认配置（4 GiB 预算 · LRU 驱逐 · 30s 温窗 · 单模型串行）。</summary>
    public static AIModelRegistryOptions Default {
        get { return new AIModelRegistryOptions(); }
    }
}
