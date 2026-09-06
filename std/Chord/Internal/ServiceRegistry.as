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
            if (previous != null) {
                // 恢复旧条目为当前：撤销「标记死亡」并重新挂载。旧实现保留
                // `!previous._dead` 守卫——但 previous 恰在此 Provide 时被标死
                //（阴影链构造），守卫恒假 → 恢复分支永不执行 → 撤销退化为
                // Remove（Provide_RevertRestoresPrevious 语义违背，实证：'
                // 撤销第二个 Provide 后 GetService 应回 v1 却得 null'）。
                previous._dead = false;
                _entries[name] = previous;
            } else {
                _entries.Remove(name);
            }
        });
    }
}
