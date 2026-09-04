// PendingTone —— 依赖准入挂起记录（RFC 045 D12）。
//
// 音的 Requires 未全部就绪时，子上下文保持 Pending（不执行 Apply）；
// 本记录登记于安装方上下文，任一依赖服务在子树内变化时重评——
// 全部就绪 → 执行 Apply 并级联启动。三种安装形态分别承载：
// 对象形态（_toneObj + _toneConfig）/ 函数形态（_apply）。
namespace Arc.Chord;

using Arc;
using Arc.Collections;


internal class PendingTone {
    internal ChordContext _tone;
    internal string[] _names;
    internal ITone? _toneObj;
    internal object? _toneConfig;
    internal Action<ChordContext>? _apply;
    internal bool _dead;

    internal PendingTone(ChordContext tone, string[] names, ITone? toneObj,
                         object? toneConfig, Action<ChordContext>? apply) {
        _tone = tone;
        _names = names;
        _toneObj = toneObj;
        _toneConfig = toneConfig;
        _apply = apply;
        _dead = false;
    }

    internal bool ContainsName(string name) {
        for (int i = 0; i < _names.Length; i++) {
            if (_names[i] == name) {
                return true;
            }
        }
        return false;
    }
}
