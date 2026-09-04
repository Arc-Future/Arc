// PendingInjection —— 注入记录（RFC 045 D4）。
//
// 三种状态：挂起等待（未执行，等待依赖就绪）/ 已执行（_ran，效果区间
// [start, end) 纳入所属上下文账本）/ 已丢弃（_dead）。
//
// 效果区间：执行前记录 _owner 账本计数，执行后记录终值；反应式回滚时
// 逆序撤销区间内条目（包含注入条目自身——其撤销动作为空操作）。
namespace Arc.Chord;

using Arc;
using Arc.Collections;


internal class PendingInjection {
    internal ChordContext _owner;
    internal string[] _names;
    internal bool[] _wasPresent;
    internal Action<ChordContext> _callback;
    internal bool _reactive;
    internal bool _dead;
    internal bool _ran;
    internal int _effectStart;
    internal int _effectEnd;

    internal PendingInjection(ChordContext owner, string[] names, bool[] wasPresent,
                              Action<ChordContext> callback, bool reactive) {
        _owner = owner;
        _names = names;
        _wasPresent = wasPresent;
        _callback = callback;
        _reactive = reactive;
        _dead = false;
        _ran = false;
        _effectStart = 0;
        _effectEnd = 0;
    }

    internal bool ContainsName(string name) {
        for (int i = 0; i < _names.Length; i++) {
            if (_names[i] == name) {
                return true;
            }
        }
        return false;
    }

    /// <summary>逆序撤销回调副作用区间（幂等：已撤销条目自动跳过）。</summary>
    internal void RevertEffects() {
        List<EffectEntry> targets = new List<EffectEntry>();
        for (int i = _effectStart; i < _effectEnd; i++) {
            targets.Add(_owner._effects.GetAt(i));
        }
        for (int j = targets.Count - 1; j >= 0; j--) {
            targets[j].Revert();
        }
    }
}
