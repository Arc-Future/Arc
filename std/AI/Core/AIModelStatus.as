// AIModelStatus — 模型生命周期状态（RFC 041 §7.2）。
//
// Cold → Warming → Ready 为正常加载路径；加载失败落 Failed；策略驱逐落
// Evicted（注册仍在，可再次 Acquire 重载）。AIModelHandle.Status 反映
// 句柄持有期间模型的状态。
namespace Arc.AI;

/// <summary>模型生命周期状态（RFC 041 §7.2）。</summary>
public enum AIModelStatus {
    /// <summary>未加载（注册未命中懒加载前的初态）。</summary>
    Cold,

    /// <summary>加载中（工厂执行中）。</summary>
    Warming,

    /// <summary>已加载就绪（runner 可用）。</summary>
    Ready,

    /// <summary>已被策略驱逐卸载（可再次 Acquire 重载）。</summary>
    Evicted,

    /// <summary>加载失败（可再次 Acquire 重试）。</summary>
    Failed,
}
