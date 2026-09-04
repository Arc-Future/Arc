// AIModelRegistry — 进程级模型组合根（RFC 041 §7.2 统一模型运行时）。
//
// 与 AIHost 同构（构造注入，Dispose 幂等）：注册静态声明（ModelId 唯一键，按名
// 覆盖）→ Acquire 懒加载 + 引用计数 → 归零经策略裁决卸载（立即 / 温窗保持）→
// 内存预算记账 + LRU 驱逐 → 统计/审计可读。注册表不引用任何后端包（依赖方向
// 红线）；加载经 <see cref="AIModelRegistration.Factory"/> 注入（后端包提供
// OnnxAIModelFactory/IreeAIModelFactory 适配）。单线程宿主约束（Host 驱动），
// 操作不加锁。
//
// 热卸载闭环（对齐 RFC 017 rt_library + RFC 005 ARC）：活跃 AIModelHandle 即根
// （refcount&gt;0 不可卸载）；卸载前置 = 引用归零 + 在途调用收敛（ActiveCalls，
// 等价 rt_library_call_enter/leave）+ 策略放行；动态库包卸载留待按需另立 RFC。
namespace Arc.AI;

using Arc.Collections;
using Arc.Diagnostics;

/// <summary>
/// 进程级模型注册表（RFC 041 §7.2）：注册 / 懒加载 / 引用计数 / 内存预算 /
/// 驱逐 / 统计审计的统一组合根。构造注入 <see cref="AIModelRegistryOptions"/>，
/// <see cref="Dispose"/> 幂等释放全部。
/// </summary>
public class AIModelRegistry : IDisposable {
    private AIModelRegistryOptions _options;
    private AIModelBudget _budget;
    private AIModelPolicy _policy;
    private Dictionary<string, AIModelEntry> _models;
    private AIModelRegistryEvents _events;
    private bool _disposed;

    /// <summary>构造注册表（组合根；选项须在构造前配好）。</summary>
    /// <param name="options">注册表配置（null → ArgumentNullException）。</param>
    public AIModelRegistry(AIModelRegistryOptions options) {
        if (options == null) {
            throw new ArgumentNullException("options");
        }
        _options = options;
        _budget = new AIModelBudget(options.MemoryBudgetBytes);
        _policy = new AIModelPolicy(options.Eviction, options.WarmKeepSeconds);
        _models = new Dictionary<string, AIModelEntry>();
        _events = null;
        _disposed = false;
    }

    // ── 注册 ──

    /// <summary>
    /// 注册模型静态声明。ModelId 为唯一键，重复注册按名覆盖（旧槽 runner 若已加载
    /// 先卸载）。LoadPolicy = Eager 立即加载；Warm 加载并预热常驻。
    /// </summary>
    public void Register(AIModelRegistration reg) {
        if (reg == null) {
            throw new ArgumentNullException("reg");
        }
        if (_disposed) {
            throw new ObjectDisposedException("AIModelRegistry");
        }
        if (reg.ModelId == null || reg.ModelId == "") {
            throw new ArgumentException("AIModelRegistration.ModelId is required");
        }
        AIModelEntry? previous = null;
        if (_models.TryGetValue(reg.ModelId, out previous) && previous != null) {
            if (previous.Runner != null) {
                this.UnloadEntry(previous);
            }
        }
        AIModelEntry entry = new AIModelEntry();
        entry.Registration = reg;
        _models[reg.ModelId] = entry;
        this.NotifyRegistered(reg);
        if (reg.LoadPolicy == AIModelLoadPolicy.Eager) {
            this.EnsureLoaded(entry);
        } else if (reg.LoadPolicy == AIModelLoadPolicy.Warm) {
            this.EnsureLoaded(entry);
            entry.PinnedByWarmup = true;
            entry.WarmUntilTick = 0;
            entry.LastUsedTick = Stopwatch.GetTimestamp();
        }
    }

    // ── 获取 / 预热 ──

    /// <summary>
    /// 获取模型句柄（懒加载 + 引用计数 +1）。首次命中创建底层 runner（创建成本
    /// 一次性摊销）；未注册抛 <see cref="AIModelNotAvailableException"/>；超单模型并发上限抛
    /// <see cref="AIModelException"/>；预算不足抛 <see cref="AIModelBudgetExceededException"/>；
    /// 加载失败抛 <see cref="AIModelLoadException"/> / <see cref="AIModelNotAvailableException"/>。
    /// </summary>
    /// <param name="modelId">注册的模型唯一键。</param>
    /// <returns>模型句柄（调用方负责 <see cref="AIModelHandle.Dispose"/>）。</returns>
    public AIModelHandle Acquire(string modelId) {
        if (_disposed) {
            throw new ObjectDisposedException("AIModelRegistry");
        }
        AIModelEntry? entry = null;
        if (!_models.TryGetValue(modelId, out entry) || entry == null) {
            throw new AIModelNotAvailableException("model not registered: " + modelId);
        }
        if (entry.RefCount >= _options.MaxConcurrentPerModel) {
            throw new AIModelException("model concurrency limit exceeded: " + modelId
                + " (MaxConcurrentPerModel=" + _options.MaxConcurrentPerModel + ")");
        }
        if (entry.Runner == null) {
            this.EnsureLoaded(entry);
        } else {
            entry.Hits = entry.Hits + 1;
        }
        entry.RefCount = entry.RefCount + 1;
        entry.LastUsedTick = Stopwatch.GetTimestamp();
        return new AIModelHandle(this, entry);
    }

    /// <summary>
    /// 预热：提前加载并常驻（不产生句柄；引用归零亦不卸载，预算压力下可被 LRU 驱逐）。
    /// </summary>
    /// <param name="modelId">注册的模型唯一键。</param>
    /// <param name="ct">协作式取消令牌（已取消抛 OperationCanceledException）。</param>
    public async Task WarmUpAsync(string modelId, CancellationToken ct) {
        ct.ThrowIfCancellationRequested();
        if (_disposed) {
            throw new ObjectDisposedException("AIModelRegistry");
        }
        AIModelEntry? entry = null;
        if (!_models.TryGetValue(modelId, out entry) || entry == null) {
            throw new AIModelNotAvailableException("model not registered: " + modelId);
        }
        if (entry.Runner == null) {
            this.EnsureLoaded(entry);
        }
        entry.PinnedByWarmup = true;
        entry.WarmUntilTick = 0;
        entry.LastUsedTick = Stopwatch.GetTimestamp();
        await Task.CompletedTask;
    }

    // ── 只读面（统计 / 预算审计）──

    /// <summary>内存预算（记账/统计可审计；ResidentBytes 实时反映常驻字节）。</summary>
    public AIModelBudget Budget {
        get { return _budget; }
    }

    /// <summary>当前已加载模型数（runner 非 null 的注册数）。</summary>
    public int LoadedCount {
        get {
            int count = 0;
            AIModelEntry[] values = _models.Values;
            int i = 0;
            while (i < values.Length) {
                if (values[i].Runner != null) {
                    count = count + 1;
                }
                i = i + 1;
            }
            return count;
        }
    }

    /// <summary>当前常驻字节（预算记账视图）。</summary>
    public long ResidentBytes {
        get { return _budget.ResidentBytes; }
    }

    /// <summary>取模型统计快照（加载/命中/驱逐/调用数/延迟；未注册抛 <see cref="AIModelException"/>）。</summary>
    public AIModelStats GetStats(string modelId) {
        AIModelEntry? entry = null;
        if (!_models.TryGetValue(modelId, out entry) || entry == null) {
            throw new AIModelNotAvailableException("model not registered: " + modelId);
        }
        AIModelStats stats = new AIModelStats();
        stats.Loads = entry.Loads;
        stats.Hits = entry.Hits;
        stats.Evictions = entry.Evictions;
        stats.Runs = entry.Runs;
        stats.TotalLatencyMs = entry.TotalLatencyMs;
        return stats;
    }

    /// <summary>订阅生命周期事件（注册/加载/驱逐/加载失败；null = 关闭通知）。</summary>
    public void SetEvents(AIModelRegistryEvents events) {
        _events = events;
    }

    // ── 包内消费（AIModelHandle / AIModelService 骨架）──

    /// <summary>句柄释放：refcount−1；归零且策略放行 → 卸载（在途调用收敛后）。</summary>
    internal void Release(AIModelEntry entry) {
        if (entry.RefCount > 0) {
            entry.RefCount = entry.RefCount - 1;
        }
        entry.LastUsedTick = Stopwatch.GetTimestamp();
        if (entry.RefCount == 0) {
            entry.WarmUntilTick = _policy.ComputeWarmUntil();
            if (_policy.CanUnload(entry, Stopwatch.GetTimestamp())) {
                this.UnloadEntry(entry);
            }
        }
    }

    /// <summary>在途调用计数 +1（服务骨架 Acquire 后登记；卸载前置 = 在途收敛）。</summary>
    internal void EnterCall(string modelId) {
        AIModelEntry? entry = null;
        if (_models.TryGetValue(modelId, out entry) && entry != null) {
            entry.ActiveCalls = entry.ActiveCalls + 1;
        }
    }

    /// <summary>在途调用计数 −1（服务骨架执行完成后登记；归零且可卸载 → 尝试卸载）。</summary>
    internal void ExitCall(string modelId) {
        AIModelEntry? entry = null;
        if (_models.TryGetValue(modelId, out entry) && entry != null) {
            if (entry.ActiveCalls > 0) {
                entry.ActiveCalls = entry.ActiveCalls - 1;
            }
            if (_policy.CanUnload(entry, Stopwatch.GetTimestamp())) {
                this.UnloadEntry(entry);
            }
        }
    }

    /// <summary>统计挂钩：记录一次推理调用（服务骨架 TrackUsage 消费）。</summary>
    internal void RecordRun(string modelId, long latencyMs) {
        AIModelEntry? entry = null;
        if (_models.TryGetValue(modelId, out entry) && entry != null) {
            entry.Runs = entry.Runs + 1;
            entry.TotalLatencyMs = entry.TotalLatencyMs + latencyMs;
        }
    }

    // ── 内部：加载 / 驱逐 / 卸载 ──

    /// <summary>
    /// 懒加载核心：预算检查（超限先 LRU 驱逐空闲；仍超限抛 BudgetExceeded）→
    /// 工厂创建 runner → Ready + 记账 + 事件。加载失败落 Failed（可再次 Acquire 重试）。
    /// </summary>
    private void EnsureLoaded(AIModelEntry entry) {
        if (!_budget.CanFit(entry.Registration.SizeBytes)) {
            if (_policy.Eviction == AIModelEvictionPolicy.Lru) {
                this.EvictLruIdle(entry.Registration.SizeBytes);
            }
            if (!_budget.CanFit(entry.Registration.SizeBytes)) {
                entry.Status = AIModelStatus.Failed;
                this.NotifyLoadFailed(entry.Registration);
                throw new AIModelBudgetExceededException("model exceeds memory budget: "
                    + entry.Registration.ModelId + " (need " + entry.Registration.SizeBytes
                    + " B, available " + _budget.AvailableBytes + " B)");
            }
        }
        entry.Status = AIModelStatus.Warming;
        try {
            // 逐步本地装载：字段直接调用非 Func apply 且复杂接收者链被硬拒绝
            // （RFC 008 M3；对齐 Arc/Types/Lazy.as 注释先例），逐级解引用到本地
            // Func 后再调用，规避两处编译器缺陷。
            AIModelRegistration reg = entry.Registration;
            Func<AIModelRegistration, IAIModel> factory = reg.Factory;
            entry.Runner = factory(entry.Registration);
        } catch (Exception ex) {
            entry.Status = AIModelStatus.Failed;
            this.NotifyLoadFailed(entry.Registration);
            if (ex is AIModelException) {
                // AI 异常（Budget/NotAvailable 等）原样传播，不做降级包装。
                throw ex;
            }
            string errMsg = ex != null && ex.Message != null ? ex.Message : "unknown error";
            throw new AIModelLoadException("failed to load model '" + entry.Registration.ModelId
                + "': " + errMsg, ex);
        }
        entry.Status = AIModelStatus.Ready;
        entry.Loads = entry.Loads + 1;
        _budget.AddResident(entry.Registration.SizeBytes);
        this.NotifyLoaded(entry.Registration);
    }

    /// <summary>预算压力驱逐：按 LRU 反复驱逐空闲（refcount==0 且无在途调用）已加载模型。</summary>
    private void EvictLruIdle(long needBytes) {
        while (!_budget.CanFit(needBytes)) {
            List<AIModelEntry> idle = this.CollectIdleLoaded();
            if (idle.Count == 0) {
                break;
            }
            AIModelEntry? victim = _policy.PickLruIdle(idle);
            if (victim == null) {
                break;
            }
            this.UnloadEntry(victim);
        }
    }

    /// <summary>收集空闲已加载候选（refcount==0 且无在途调用且 runner 非 null）。</summary>
    private List<AIModelEntry> CollectIdleLoaded() {
        List<AIModelEntry> idle = new List<AIModelEntry>();
        AIModelEntry[] values = _models.Values;
        int i = 0;
        while (i < values.Length) {
            AIModelEntry e = values[i];
            if (e.Runner != null && e.RefCount == 0 && e.ActiveCalls == 0) {
                idle.Add(e);
            }
            i = i + 1;
        }
        return idle;
    }

    /// <summary>卸载：Dispose runner → Evicted + 记账递减 + 事件（幂等，runner 已 null 无操作）。</summary>
    private void UnloadEntry(AIModelEntry entry) {
        if (entry.Runner != null) {
            // 本地装载后调用：接口字段直接方法调用非 Func apply（编译器缺陷，对齐
            // Arc/Types/Lazy.as 注释先例），本地接口引用调用正确。
            IAIModel runner = entry.Runner;
            runner.Dispose();
            entry.Runner = null;
        }
        entry.Status = AIModelStatus.Evicted;
        entry.WarmUntilTick = 0;
        entry.PinnedByWarmup = false;
        entry.Evictions = entry.Evictions + 1;
        _budget.RemoveResident(entry.Registration.SizeBytes);
        this.NotifyEvicted(entry.Registration);
    }

    // ── 事件 ──

    private void NotifyRegistered(AIModelRegistration reg) {
        if (_events != null && _events.OnModelRegistered != null) {
            _events.OnModelRegistered(reg);
        }
    }

    private void NotifyLoaded(AIModelRegistration reg) {
        if (_events != null && _events.OnModelLoaded != null) {
            _events.OnModelLoaded(reg);
        }
    }

    private void NotifyEvicted(AIModelRegistration reg) {
        if (_events != null && _events.OnModelEvicted != null) {
            _events.OnModelEvicted(reg);
        }
    }

    private void NotifyLoadFailed(AIModelRegistration reg) {
        if (_events != null && _events.OnLoadFailed != null) {
            _events.OnLoadFailed(reg);
        }
    }

    // ── 释放 ──

    /// <summary>释放全部底层 runner（幂等：重复调用无操作）。</summary>
    public void Dispose() {
        if (_disposed) {
            return;
        }
        _disposed = true;
        AIModelEntry[] values = _models.Values;
        int i = 0;
        while (i < values.Length) {
            AIModelEntry e = values[i];
            if (e.Runner != null) {
                // 本地装载后调用（编译器缺陷规避，对齐 UnloadEntry 先例）。
                IAIModel runner = e.Runner;
                runner.Dispose();
                e.Runner = null;
                e.Status = AIModelStatus.Evicted;
                e.RefCount = 0;
                e.PinnedByWarmup = false;
                e.WarmUntilTick = 0;
                _budget.RemoveResident(e.Registration.SizeBytes);
            }
            i = i + 1;
        }
    }
}
