// EffectRegistry —— 副作用账本（RFC 045 D2/D6）。
//
// 内核唯一副作用登记表：Add 立即执行回调并登记撤销句柄；RevertAll 按
// LIFO 逆序批量撤销（异常安全：单个撤销失败不中断其余）；TransferTo 供
// 事务 Commit 将效果原子迁移到父上下文。
namespace Arc.Chord;

using Arc;
using Arc.Collections;


internal class EffectRegistry {
    private List<EffectEntry> _entries;

    internal EffectRegistry() {
        _entries = new List<EffectEntry>();
    }

    internal int Count {
        get {
            return _entries.Count;
        }
    }

    internal EffectEntry GetAt(int index) {
        return _entries[index];
    }

    /// <summary>登记副作用并立即执行回调；回调抛异常则不保留条目并向调用方传播。</summary>
    internal EffectEntry Add(Func<IDisposable> callback) {
        EffectEntry entry = new EffectEntry(this, callback);
        _entries.Add(entry);
        try {
            entry.Run();
        } catch (Exception e) {
            _entries.RemoveAt(_entries.Count - 1);
            throw e;
        }
        return entry;
    }

    internal void Remove(EffectEntry entry) {
        for (int i = 0; i < _entries.Count; i++) {
            if (_entries[i] == entry) {
                _entries.RemoveAt(i);
                return;
            }
        }
    }

    /// <summary>LIFO 逆序批量撤销；单个撤销异常不中断其余（异常安全）。</summary>
    internal void RevertAll() {
        for (int i = _entries.Count - 1; i >= 0; i--) {
            try {
                _entries[i].RevertInternal();
            } catch (Exception) {
                // 异常安全：继续撤销其余条目
            }
        }
        _entries.Clear();
    }

    /// <summary>把全部条目迁移到目标注册表（事务 Commit）；条目撤销句柄保持可用。</summary>
    internal void TransferTo(EffectRegistry target) {
        for (int i = 0; i < _entries.Count; i++) {
            EffectEntry entry = _entries[i];
            entry._owner = target;
            target._entries.Add(entry);
        }
        _entries.Clear();
    }
}
