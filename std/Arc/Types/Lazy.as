// Lazy<T> — deferred initialization (C# System.Lazy<T> essence).
//
// Single idiom (RFC 002): Lock + Monitor around factory; no LazyThreadSafetyMode dual track.
// Maturity: Stable 最小面 — main-thread + worker/concurrent first eval + Lazy<string>
// falsifiable (LazyTests / lazy_e2e). No Lazy(T) value-ctor; no mode enum dual track.
// No unsafe; pure Arc (Monitor.Enter/Exit + try/finally).

namespace Arc;

using Arc.Threading;

/// <summary>
/// Lazy initialization wrapper. Factory runs at most once under lock;
/// <c>Value</c> is cached thereafter (calling thread or workers).
/// </summary>
/// <remarks>
/// 成熟度：Stable 最小面（单线程 / worker / 并发首次求值 + <c>Lazy&lt;string&gt;</c> 可证伪）。
/// 实现使用 <c>Lock</c>+<c>Monitor</c> 单惯用法，对齐 C# ExecutionAndPublication 精华；
/// <b>无</b> <c>LazyThreadSafetyMode</c> 枚举双轨；无预置值 <c>Lazy(T)</c> ctor。
/// </remarks>
public class Lazy<T> {
    private Func<T> _valueFactory;
    private T _cachedValue;
    private bool _isValueCreated;
    private Lock _sync;

    public Lazy(Func<T> factory) {
        _valueFactory = factory;
        _isValueCreated = false;
        _sync = new Lock();
    }

    /// <summary>Whether the factory has already produced a value.</summary>
    public bool IsValueCreated { get { return _isValueCreated; } }

    /// <summary>Cached value; first access runs the factory under lock.</summary>
    public T Value {
        get {
            Monitor.Enter(_sync);
            try {
                if (!_isValueCreated) {
                    // Local load: field call `_valueFactory()` is not Func apply.
                    Func<T> f = _valueFactory;
                    _cachedValue = f();
                    _isValueCreated = true;
                }
                return _cachedValue;
            } finally {
                Monitor.Exit(_sync);
            }
        }
    }
}
