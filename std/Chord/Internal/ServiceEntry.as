// ServiceEntry —— 服务条目（RFC 045 D3/D14）。
//
// _dead 标记支持「后写优先」阴影覆盖：旧条目被覆盖后保持条目但标记死亡，
// 撤销恢复时据此判断旧值是否仍有效。工厂条目（_factory）首次解析时构造
// 并缓存（MEDI 工厂语义同构，按需构造）。
namespace Arc.Chord;

internal class ServiceEntry {
    internal string _name;
    internal object? _instance;
    internal Func<object?>? _factory;
    internal bool _materialized;
    internal bool _dead;

    internal ServiceEntry(string name, object? instance) {
        _name = name;
        _instance = instance;
        _factory = null;
        _materialized = true;
        _dead = false;
    }

    internal ServiceEntry(string name, Func<object?> factory) {
        _name = name;
        _instance = null;
        _factory = factory;
        _materialized = false;
        _dead = false;
    }

    /// <summary>解析条目值（工厂条目首次解析时构造并缓存）。</summary>
    internal object? Resolve() {
        if (_factory != null && !_materialized) {
            _instance = _factory();
            _materialized = true;
        }
        return _instance;
    }
}
