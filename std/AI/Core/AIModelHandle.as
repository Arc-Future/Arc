// AIModelHandle — 模型句柄（RFC 041 §7.2 引用计数）。
//
// Acquire 返回的强引用根：持有期间注册表对该模型保持常驻（refcount&gt;0，不可
// 卸载/驱逐）。Dispose = refcount-1；归零且策略放行（温窗已过 / 立即卸载）→
// 进入可卸载候选（注册表按策略卸载并 Dispose 底层 runner）。活跃句柄即注册表
// 持强引用的根（对齐 RFC 099/102 热卸载「活跃 AIModelHandle 为根」语义）。
namespace Arc.AI;

/// <summary>
/// 模型句柄（RFC 041 §7.2）：Acquire 产出的强引用，持有期间模型常驻。
/// <see cref="Dispose"/> 幂等（refcount−1；归零且策略放行 → 可卸载）。
/// </summary>
public class AIModelHandle : IDisposable {
    private AIModelRegistry _registry;
    private AIModelEntry _entry;
    private bool _disposed;

    /// <summary>由注册表 Acquire 构造（包内）。</summary>
    internal AIModelHandle(AIModelRegistry registry, AIModelEntry entry) {
        _registry = registry;
        _entry = entry;
        _disposed = false;
    }

    /// <summary>模型唯一键（与注册 ModelId 一致）。</summary>
    public string ModelId {
        get { return _entry.Registration.ModelId; }
    }

    /// <summary>底层推理运行器（句柄有效期间非 null；语义面仍经 <see cref="IAIModel"/> 消费）。</summary>
    public IAIModel Runner {
        get { return _entry.Runner; }
    }

    /// <summary>模型生命周期状态（有效句柄通常为 <see cref="AIModelStatus.Ready"/>）。</summary>
    public AIModelStatus Status {
        get { return _entry.Status; }
    }

    /// <summary>释放句柄：refcount−1；归零且策略放行 → 可卸载（幂等，重复调用无操作）。</summary>
    public void Dispose() {
        if (_disposed) {
            return;
        }
        _disposed = true;
        _registry.Release(_entry);
    }
}
