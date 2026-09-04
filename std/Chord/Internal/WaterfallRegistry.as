// WaterfallRegistry —— 瀑布管道表（RFC 045 D5.1）。
//
// 单上下文内订阅与触发：handler(payload, next) 按注册序串联（prepend
// 插队），next 显式委托下一环；不调 next 即拦截（短路）；无订阅时
// next 为恒等（原样返回）。
namespace Arc.Chord;

using Arc;
using Arc.Collections;


internal class WaterfallRegistry {
    private Dictionary<string, List<WaterfallEntry>> _entries;

    internal WaterfallRegistry() {
        _entries = new Dictionary<string, List<WaterfallEntry>>();
    }

    /// <summary>订阅瀑布；返回退订句柄（惰性移除：置 dead，Run 尾部紧凑）。</summary>
    internal IDisposable Add(string name, Func<object?, Func<object?, object?>, object?> handler) {
        return this.Add(name, handler, false);
    }

    /// <summary>订阅瀑布（prepend 插队到队首）。</summary>
    internal IDisposable Add(string name, Func<object?, Func<object?, object?>, object?> handler, bool prepend) {
        List<WaterfallEntry> list = new List<WaterfallEntry>();
        if (_entries.ContainsKey(name)) {
            list = _entries[name];
        } else {
            _entries.Add(name, list);
        }
        WaterfallEntry entry = new WaterfallEntry(handler);
        if (prepend) {
            list.Insert(0, entry);
        } else {
            list.Add(entry);
        }
        return new DisposableAction(() => {
            entry._dead = true;
        });
    }

    /// <summary>触发瀑布：快照串联，容忍回调中退订/新增订阅。</summary>
    internal object Run(string name, object payload) {
        if (!_entries.ContainsKey(name)) {
            return payload;
        }
        List<WaterfallEntry> list = _entries[name];
        List<WaterfallEntry> snapshot = new List<WaterfallEntry>();
        for (int i = 0; i < list.Count; i++) {
            snapshot.Add(list[i]);
        }
        object result = this.RunAt(snapshot, 0, payload);
        this.Compact(list);
        if (list.Count == 0 && _entries.ContainsKey(name)) {
            _entries.Remove(name);
        }
        return result;
    }

    /// <summary>从第 index 环起串联：末端越界即恒等返回（next 链底）。</summary>
    private object RunAt(List<WaterfallEntry> list, int index, object payload) {
        if (index >= list.Count) {
            return payload;
        }
        WaterfallEntry entry = list[index];
        if (entry._dead) {
            return this.RunAt(list, index + 1, payload);
        }
        int nextIndex = index + 1;
        return entry._handler(payload, (p) => this.RunAt(list, nextIndex, p));
    }

    /// <summary>惰性清除已退订条目（逆序扫描）。</summary>
    private void Compact(List<WaterfallEntry> list) {
        for (int i = list.Count - 1; i >= 0; i--) {
            if (list[i]._dead) {
                list.RemoveAt(i);
            }
        }
    }
}
