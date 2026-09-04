// ServiceRegistry —— 本地服务注册表（RFC 045 D3）。
//
// 可撤销阴影注册：Provide 覆盖本地同名条目（旧条目标记死亡并保留引用）；
// 撤销句柄执行时若自身仍是当前条目则恢复旧条目，否则 no-op（后写优先）。
namespace Arc.Chord;

using Arc;
using Arc.Collections;


internal class ServiceRegistry {
    private Dictionary<string, ServiceEntry> _entries;

    internal ServiceRegistry() {
        _entries = new Dictionary<string, ServiceEntry>();
    }

    internal object? Get(string name) {
        if (_entries.ContainsKey(name)) {
            ServiceEntry entry = _entries[name];
            if (!entry._dead) {
                return entry.Resolve();
            }
        }
        return null;
    }

    internal bool Has(string name) {
        if (_entries.ContainsKey(name)) {
            return !_entries[name]._dead;
        }
        return false;
    }

    /// <summary>提供/覆盖服务；返回撤销句柄（撤销恢复旧条目或移除自身）。</summary>
    internal IDisposable Provide(string name, object? instance) {
        return this.ProvideEntry(new ServiceEntry(name, instance));
    }

    /// <summary>按工厂提供/覆盖服务：首次解析时构造并缓存（RFC 045 D14）。</summary>
    internal IDisposable ProvideFactory(string name, Func<object?> factory) {
        return this.ProvideEntry(new ServiceEntry(name, factory));
    }

    private IDisposable ProvideEntry(ServiceEntry entry) {
        string name = entry._name;
        ServiceEntry? previous = null;
        if (_entries.ContainsKey(name)) {
            previous = _entries[name];
            previous._dead = true;
        }
        _entries[name] = entry;
        return new DisposableAction(() => {
            if (entry._dead) {
                return;   // 已被后续 Provide 覆盖：后写优先，撤销 no-op
            }
            entry._dead = true;
            if (previous != null && !previous._dead) {
                _entries[name] = previous;
            } else {
                _entries.Remove(name);
            }
        });
    }
}
