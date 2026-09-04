// ScopeStatus —— 作用域状态机（RFC 045 D1/D7/D9）。
namespace Arc.Chord;

/// <summary>
/// 作用域状态：
/// Pending —— 已创建未启动（父未启动）；
/// Active  —— 已启动运行中；
/// Failed  —— 安装/启动失败（副作用已全部回滚，Error 承载原因）；
/// Disposed—— 已卸载。
/// </summary>
public enum ScopeStatus {
    Pending,
    Active,
    Failed,
    Disposed,
}
