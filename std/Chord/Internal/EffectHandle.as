// EffectHandle —— 单个副作用句柄（RFC 045 D2 单句柄撤销）。
namespace Arc.Chord;

using Arc;
using Arc.Collections;


internal class EffectHandle : IDisposable {
    private EffectEntry _entry;
    private bool _disposed;

    internal EffectHandle(EffectEntry entry) {
        _entry = entry;
        _disposed = false;
    }

    public void Dispose() {
        if (_disposed) {
            return;
        }
        _disposed = true;
        _entry.Revert();
    }
}
