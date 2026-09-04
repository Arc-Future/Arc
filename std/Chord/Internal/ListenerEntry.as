// ListenerEntry —— 事件监听条目（RFC 045 D5）。
//
// _dead 标记支持快照遍历中的惰性退订：Emit 先快照监听列表，回调中途
// 退订（置 dead）不影响本轮其余监听；Once 监听触发后立即置 dead，
// 由 Emit 尾部紧凑清除。
namespace Arc.Chord;

using Arc;
using Arc.Collections;


internal class ListenerEntry {
    internal bool _dead;
    internal bool _once;
    internal Action<object?> _callback;

    internal ListenerEntry(Action<object?> callback, bool once) {
        _callback = callback;
        _once = once;
        _dead = false;
    }

    internal void Invoke(object? payload) {
        _callback(payload);
    }
}
