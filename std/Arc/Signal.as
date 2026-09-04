// RFC 037 D4.4 / RFC 037 D7: Arc — Signal<T> 响应式信号。
//
// **命名空间归属**：Signal<T> 是通用响应式原语，归 `Arc` 根命名空间。
//
// 覆盖 WPF INotifyPropertyChanging + INotifyPropertyChanged 全语义——
//   - OnChanging(handler) 替代 PropertyChanging + Cancel
//   - OnChanged(handler) 替代 PropertyChanged + (old, new) 二元组
//   - TrySet(value) 可拒绝的 set
//   - Set(value) 不可拒绝的 set（兼容旧 API）
//   - Subscribe(handler) 仅传新值的轻量订阅
//
// **实现状态**：M1 类型骨架 ✅ · M2-A Subscription 令牌（int）✅
//   · M-D0 前置修复 ✅（统一 token 空间：全局递增 token + 并行登记表精确定位，
//     跨双 handler 表（_changingHandlers/_changedHandlers 各自从 0 编号）的
//     偏移误清空缺陷已修复；公开 API 签名与语义不变）

namespace Arc;

using Arc.Collections;

/// <summary>
/// 响应式信号——Arc 推荐的命令式数据源。
/// </summary>
public class Signal<T> {
    public T Value;

    // 双 handler 表（保持原有结构）：
    //   _changingHandlers：变更前校验（可拒绝），Func<T,T,bool>
    //   _changedHandlers：变更后通知（old, new 二元组），Action<T,T>
    private List<Func<T, T, bool>> _changingHandlers;
    private List<Action<T, T>> _changedHandlers;

    // M-D0 统一 token 空间：
    //   _nextToken：全局递增计数器，每次订阅取 _nextToken++，token 全局唯一；
    //   _tokenKind / _tokenIndex：以 token 为下标的并行登记表，
    //     记录该 token 对应的（handler 种类，在对应表中的下标），
    //     使 Unsubscribe(token) 能精确定位到唯一一个处理函数，杜绝跨表误清空。
    private int _nextToken;
    private List<int> _tokenKind;    // 0 = _changingHandlers 表；1 = _changedHandlers 表
    private List<int> _tokenIndex;   // 在对应表中的下标

    public Signal() {
        this.Value = default(T);
        _changingHandlers = new List<Func<T, T, bool>>();
        _changedHandlers = new List<Action<T, T>>();
        _nextToken = 0;
        _tokenKind = new List<int>();
        _tokenIndex = new List<int>();
    }

    public Signal(T initial) {
        this.Value = initial;
        _changingHandlers = new List<Func<T, T, bool>>();
        _changedHandlers = new List<Action<T, T>>();
        _nextToken = 0;
        _tokenKind = new List<int>();
        _tokenIndex = new List<int>();
    }

    // ── 订阅 API（M2-A：返回 int 令牌；M-D0：令牌全局递增、跨表唯一） ──

    public int OnChanging(Func<T, T, bool> handler) {
        // Lazy-init：防御零初始化 / 未走完整 ctor 的对象（真实 `__ctor` 会初始化 List）。
        if (_changingHandlers == null) {
            _changingHandlers = new List<Func<T, T, bool>>();
        }
        this.EnsureTokenRegistry();
        int index = _changingHandlers.Count;
        _changingHandlers.Add(handler);
        // M-D0：取全局递增 token，登记 (kind=0=changing 表, index)，保证跨表唯一。
        int token = _nextToken;
        _nextToken = _nextToken + 1;
        _tokenKind.Add(0);
        _tokenIndex.Add(index);
        return token;
    }

    public int OnChanged(Action<T, T> handler) {
        if (_changedHandlers == null) {
            _changedHandlers = new List<Action<T, T>>();
        }
        this.EnsureTokenRegistry();
        int index = _changedHandlers.Count;
        _changedHandlers.Add(handler);
        // M-D0：取全局递增 token，登记 (kind=1=changed 表, index)，保证跨表唯一。
        int token = _nextToken;
        _nextToken = _nextToken + 1;
        _tokenKind.Add(1);
        _tokenIndex.Add(index);
        return token;
    }

    public int Subscribe(Action<T> handler) {
        return this.OnChanged((_, newValue) => handler(newValue));
    }

    // M2-A：按令牌取消订阅（标记 null 以跳过）。
    // M-D0：token 全局唯一，经登记表精确定位到唯一一个处理函数，只清该订阅，
    //       绝不碰其他订阅（修复旧实现双表各自从 0 编号导致的跨表误清空）。
    public void Unsubscribe(int token) {
        if (token >= 0 && _tokenKind != null && token < _tokenKind.Count) {
            int kind = _tokenKind[token];
            int index = _tokenIndex[token];
            if (kind == 0) {
                if (_changingHandlers != null && index < _changingHandlers.Count) {
                    _changingHandlers[index] = null;
                }
            } else {
                if (_changedHandlers != null && index < _changedHandlers.Count) {
                    _changedHandlers[index] = null;
                }
            }
        }
    }

    // ── 设值 API ──

    public bool TrySet(T newValue) {
        T old = this.Value;

        if (_changingHandlers != null) {
            foreach (var handler in _changingHandlers) {
                if (handler != null) {
                    if (!handler(old, newValue)) {
                        return false;
                    }
                }
            }
        }

        this.Value = newValue;
        this.NotifyChanged(old, newValue);
        return true;
    }

    public void Set(T newValue) {
        this.TrySet(newValue);
    }

    private void NotifyChanged(T oldValue, T newValue) {
        if (_changedHandlers == null) { return; }
        foreach (var handler in _changedHandlers) {
            if (handler != null) {
                handler(oldValue, newValue);
            }
        }
    }

    // M-D0：统一 token 空间登记表 Lazy-init（防御零初始化 / 未走完整 ctor 的对象）。
    private void EnsureTokenRegistry() {
        if (_tokenKind == null) {
            _tokenKind = new List<int>();
        }
        if (_tokenIndex == null) {
            _tokenIndex = new List<int>();
        }
    }
}
