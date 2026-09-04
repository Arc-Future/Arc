// EventEmitter —— 事件表（RFC 045 D5）。
//
// 仅承载单上下文的监听注册与触发；跨上下文广播（后代/祖先）由 ChordContext
// 的 Emit/Bubble 负责。prepend 插入队首（优先级）；Once 触发即退订。
namespace Arc.Chord;

using Arc;
using Arc.Collections;


internal class EventEmitter {
    private Dictionary<string, List<ListenerEntry>> _listeners;

    internal EventEmitter() {
        _listeners = new Dictionary<string, List<ListenerEntry>>();
    }

    /// <summary>订阅事件；返回退订句柄（惰性移除：置 dead，Emit 尾部紧凑）。</summary>
    internal IDisposable Add(string name, Action<object?> listener, bool prepend, bool once) {
        List<ListenerEntry> list = new List<ListenerEntry>();
        if (_listeners.ContainsKey(name)) {
            list = _listeners[name];
        } else {
            list = new List<ListenerEntry>();
            _listeners.Add(name, list);
        }
        ListenerEntry entry = new ListenerEntry(listener, once);
        if (prepend) {
            list.Insert(0, entry);
        } else {
            list.Add(entry);
        }
        return new DisposableAction(() => {
            entry._dead = true;
        });
    }

    /// <summary>触发事件：快照遍历，容忍回调中退订/新增监听；Once 自退订。</summary>
    internal void Emit(string name, object? payload) {
        if (!_listeners.ContainsKey(name)) {
            return;
        }
        List<ListenerEntry> list = _listeners[name];
        List<ListenerEntry> snapshot = new List<ListenerEntry>();
        for (int i = 0; i < list.Count; i++) {
            snapshot.Add(list[i]);
        }
        for (int i = 0; i < snapshot.Count; i++) {
            ListenerEntry entry = snapshot[i];
            if (entry._dead) {
                continue;
            }
            if (entry._once) {
                entry._dead = true;
            }
            entry.Invoke(payload);
        }
        this.Compact(list);
        if (list.Count == 0 && _listeners.ContainsKey(name)) {
            _listeners.Remove(name);
        }
    }

    /// <summary>惰性清除已退订条目（逆序扫描，退订集中在尾部与 Once 触发点）。</summary>
    private void Compact(List<ListenerEntry> list) {
        for (int i = list.Count - 1; i >= 0; i--) {
            if (list[i]._dead) {
                list.RemoveAt(i);
            }
        }
    }
}
