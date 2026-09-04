// DisposableAction —— 将 Action 包装为幂等 IDisposable（RFC 045 D2 撤销句柄载体）。
//
// 内核大量 API 以「返回撤销句柄」表达可逆性；用户自定义副作用撤销逻辑
// （如 Effect 回调返回的清理动作）同样经本类型承载。
namespace Arc.Chord;

using Arc;
using Arc.Collections;


/// <summary>
/// 将 Action 包装为可执行一次的 IDisposable；重复 Dispose 安全（仅首次执行）。
/// </summary>
public class DisposableAction : IDisposable {
    private Action _action;
    private bool _disposed;

    /// <param name="action">撤销动作；null 视为空操作。</param>
    public DisposableAction(Action? action) {
        _action = action != null ? action : new Action(() => { });
    }

    /// <summary>执行撤销动作（幂等：仅首次生效）。</summary>
    public void Dispose() {
        if (_disposed) {
            return;
        }
        _disposed = true;
        _action();
    }
}
