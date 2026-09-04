// EffectEntry —— 副作用账本条目（RFC 045 D2）。
//
// 一个条目 = 一次已登记副作用：注册时立即执行 _callback() 取得撤销句柄
// _disposer；撤销（Revert）只执行一次（幂等）。条目归属 _owner 注册表，
// 事务 Commit 时经 _owner 重定向实现效果迁移。
namespace Arc.Chord;

using Arc;
using Arc.Collections;


internal class EffectEntry {
    internal EffectRegistry _owner;
    internal Func<IDisposable> _callback;
    internal IDisposable? _disposer;
    internal bool _dead;

    internal EffectEntry(EffectRegistry owner, Func<IDisposable> callback) {
        _owner = owner;
        _callback = callback;
        _disposer = null;
        _dead = false;
    }

    /// <summary>注册时立即执行回调，取得撤销句柄。</summary>
    internal void Run() {
        _disposer = _callback();
    }

    /// <summary>撤销本副作用（幂等）；从所属注册表摘除。</summary>
    internal void Revert() {
        if (_dead) {
            return;
        }
        _dead = true;
        if (_disposer != null) {
            _disposer.Dispose();
        }
        if (_owner != null) {
            _owner.Remove(this);
            _owner = null;
        }
    }

    /// <summary>批量撤销路径：仅执行撤销动作，不操作列表（由调用方统一清理）。</summary>
    internal void RevertInternal() {
        if (_dead) {
            return;
        }
        _dead = true;
        if (_disposer != null) {
            _disposer.Dispose();
        }
    }
}
