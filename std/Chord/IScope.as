// IScope —— 作用域契约（RFC 045 D1）。
//
// 每个 ChordContext 绑定一个 Scope；Scope 记录身份（Uid/Name）、生命周期状态、
// 音配置与失败原因。状态迁移由 ChordContext 驱动（Start/Dispose/失败回滚），
// 使用者只读观察。
namespace Arc.Chord;

/// <summary>
/// 作用域契约——上下文的生命周期记录。
/// </summary>
public interface IScope {
    /// <summary>全局唯一标识（进程内递增）。</summary>
    int Uid { get; }

    /// <summary>作用域名（root / tone / transaction）。</summary>
    string Name { get; }

    /// <summary>生命周期状态。</summary>
    ScopeStatus Status { get; }

    /// <summary>音配置对象（Tone(apply, config) 传入；根/事务为 null）。</summary>
    object? Config { get; }

    /// <summary>失败原因（Status == Failed 时有值）。</summary>
    string? Error { get; }
}
