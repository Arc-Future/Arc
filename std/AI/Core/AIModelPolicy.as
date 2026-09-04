// AIModelPolicy — 驱逐/温窗策略裁决（RFC 041 §7.2）。
//
// 与 AIModelBudget（记账）正交：本类只裁决——引用归零后是否可卸载（温窗判定）、
// 预算压力下驱逐哪个空闲模型（LRU）。裁决基于注册表内部模型槽状态，包内消费。
namespace Arc.AI;

using Arc.Collections;
using Arc.Diagnostics;

/// <summary>
/// 模型驱逐/温窗策略（RFC 041 §7.2）。由注册表按选项构造；裁决方法包内消费，
/// 只读面（<see cref="Eviction"/> / <see cref="WarmKeepSeconds"/>）公开。
/// </summary>
public class AIModelPolicy {
    private AIModelEvictionPolicy _eviction;
    private int _warmKeepSeconds;

    /// <summary>由注册表按选项构造。</summary>
    internal AIModelPolicy(AIModelEvictionPolicy eviction, int warmKeepSeconds) {
        _eviction = eviction;
        _warmKeepSeconds = warmKeepSeconds < 0 ? 0 : warmKeepSeconds;
    }

    /// <summary>驱逐策略（None = 不驱逐 / Lru = 预算压力驱逐空闲）。</summary>
    public AIModelEvictionPolicy Eviction {
        get { return _eviction; }
    }

    /// <summary>引用归零后的温窗保持秒数（0 = 立即卸载）。</summary>
    public int WarmKeepSeconds {
        get { return _warmKeepSeconds; }
    }

    /// <summary>把 <paramref name="seconds"/> 换算为计时器 tick（LRU 时间轴）。</summary>
    private static long SecondsToTicks(int seconds) {
        return ((long)seconds) * Stopwatch.Frequency;
    }

    /// <summary>计算引用归零后应设置的温窗截止 tick（0 = 立即卸载）。</summary>
    internal long ComputeWarmUntil() {
        if (_warmKeepSeconds <= 0) {
            return 0;
        }
        return Stopwatch.GetTimestamp() + AIModelPolicy.SecondsToTicks(_warmKeepSeconds);
    }

    /// <summary>
    /// 卸载判定：引用归零 + 非预热常驻 + （无需温窗 或 温窗已过）。
    /// 在途调用（ActiveCalls &gt; 0）亦禁止卸载（在途收敛，等价 rt_library_call_enter/leave）。
    /// </summary>
    internal bool CanUnload(AIModelEntry entry, long nowTick) {
        if (entry.RefCount > 0) {
            return false;
        }
        if (entry.ActiveCalls > 0) {
            return false;
        }
        if (entry.PinnedByWarmup) {
            return false;
        }
        if (entry.WarmUntilTick == 0) {
            return true;
        }
        return nowTick >= entry.WarmUntilTick;
    }

    /// <summary>从空闲（引用归零）候选中选最久未用者（LRU 排序键 LastUsedTick）。</summary>
    internal AIModelEntry? PickLruIdle(List<AIModelEntry> idle) {
        AIModelEntry? best = null;
        int n = idle.Count;
        int i = 0;
        while (i < n) {
            AIModelEntry candidate = idle[i];
            if (best == null || candidate.LastUsedTick < best.LastUsedTick) {
                best = candidate;
            }
            i = i + 1;
        }
        return best;
    }
}
