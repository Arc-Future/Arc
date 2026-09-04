// AIModelStats — 模型统计快照（RFC 041 §7.2 统计/审计）。
//
// GetStats(modelId) 返回值（新对象，读时快照）：加载/命中/驱逐计数 + 推理调用
// 数与累计延迟（服务骨架 TrackUsage 挂钩记账）。AvgLatencyMs 为推导均值。
namespace Arc.AI;

/// <summary>模型统计快照（RFC 041 §7.2；<see cref="AIModelRegistry.GetStats"/>
/// 返回）。字段为累积计数，读时拷贝快照。</summary>
public class AIModelStats {
    /// <summary>累计加载次数。</summary>
    public long Loads;

    /// <summary>Acquire 命中已加载次数（懒加载摊销收益）。</summary>
    public long Hits;

    /// <summary>累计驱逐次数。</summary>
    public long Evictions;

    /// <summary>累计推理调用数（服务骨架 TrackUsage 记账）。</summary>
    public long Runs;

    /// <summary>累计推理延迟（毫秒）。</summary>
    public long TotalLatencyMs;

    public AIModelStats() {
        this.Loads = 0;
        this.Hits = 0;
        this.Evictions = 0;
        this.Runs = 0;
        this.TotalLatencyMs = 0;
    }

    /// <summary>平均单次推理延迟（毫秒；Runs 为 0 时返回 0）。</summary>
    public long AvgLatencyMs {
        get {
            if (this.Runs <= 0) {
                return 0;
            }
            return this.TotalLatencyMs / this.Runs;
        }
    }
}
