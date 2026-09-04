// ConfigStore —— 本地配置表（RFC 045 D2 配置撤销）。
//
// 读取语义与缺失语义合并：存储 null 值等价于未存储（文档声明）。
// Set 返回撤销句柄：恢复旧值或移除自身（事务/作用域释放时逆序恢复）。
namespace Arc.Chord;

using Arc;
using Arc.Collections;


internal class ConfigStore {
    private Dictionary<string, object?> _values;

    internal ConfigStore() {
        _values = new Dictionary<string, object?>();
    }

    internal object? Get(string name) {
        if (_values.ContainsKey(name)) {
            return _values[name];
        }
        return null;
    }

    internal bool Has(string name) {
        return _values.ContainsKey(name);
    }

    internal IDisposable Set(string name, object? value) {
        bool had = _values.ContainsKey(name);
        object? old = had ? _values[name] : null;
        _values[name] = value;
        return new DisposableAction(() => {
            if (had) {
                _values[name] = old;
            } else {
                _values.Remove(name);
            }
        });
    }
}
