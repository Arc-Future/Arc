// AIModelEntry — 注册表内部模型槽（RFC 041 §7.2 生命周期状态机）。
//
// 每注册模型一个槽位：持有注册声明 + 懒加载的 runner + 引用计数 + 在途调用
// 计数（卸载前置：在途调用收敛，等价 rt_library_call_enter/leave 语义）+ LRU
// 时间戳 + 温窗截止 + 统计。internal：仅注册表/句柄/服务骨架同包消费。
namespace Arc.AI;

/// <summary>注册表内部模型槽（RFC 041 §7.2；包内实现细节，不对外暴露）。</summary>
internal class AIModelEntry {
    /// <summary>注册声明（不可变）。</summary>
    public AIModelRegistration Registration;

    /// <summary>懒加载的底层推理运行器；null = 未加载（Cold/Failed/Evicted）。</summary>
    public IAIModel Runner;

    /// <summary>生命周期状态。</summary>
    public AIModelStatus Status;

    /// <summary>活跃句柄引用计数（Acquire +1 / Handle.Dispose −1）。</summary>
    public int RefCount;

    /// <summary>在途调用计数（服务骨架 RecordRunStart/End；归零卸载前置）。</summary>
    public int ActiveCalls;

    /// <summary>最近使用时间戳（Stopwatch.GetTimestamp；LRU 排序键）。</summary>
    public long LastUsedTick;

    /// <summary>温窗截止时间戳（0 = 引用归零即卸载；&gt;0 = 温窗内保持常驻）。</summary>
    public long WarmUntilTick;

    /// <summary>预热常驻标记：WarmUpAsync 加载后不因引用归零卸载（预算压力可 LRU 驱逐）。</summary>
    public bool PinnedByWarmup;

    /// <summary>累计加载次数。</summary>
    public long Loads;

    /// <summary>Acquire 命中已加载次数。</summary>
    public long Hits;

    /// <summary>累计驱逐次数。</summary>
    public long Evictions;

    /// <summary>累计推理调用数（服务骨架统计挂钩）。</summary>
    public long Runs;

    /// <summary>累计推理延迟（毫秒；经 Runs 求均值）。</summary>
    public long TotalLatencyMs;

    public AIModelEntry() {
        this.Registration = null;
        this.Runner = null;
        this.Status = AIModelStatus.Cold;
        this.RefCount = 0;
        this.ActiveCalls = 0;
        this.LastUsedTick = 0;
        this.WarmUntilTick = 0;
        this.PinnedByWarmup = false;
        this.Loads = 0;
        this.Hits = 0;
        this.Evictions = 0;
        this.Runs = 0;
        this.TotalLatencyMs = 0;
    }
}
