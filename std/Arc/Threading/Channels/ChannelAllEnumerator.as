// ChannelAllEnumerator<T> — ReadAllAsync 异步枚举游标（RFC 046）。
namespace Arc.Threading.Channels;

using Arc;
using Arc.Collections;

/// <summary>
/// ReadAllAsync 的异步枚举游标——IAsyncEnumerator 面：循环驱动读取委托
/// 直至终结排尽（ChannelClosedException 归为序列结束）；终结 error /
/// 取消原样传播。单游标单次枚举，非线程安全（对齐 IAsyncEnumerator 契约）。
///
/// await 作用于局部变量（`Task&lt;T&gt; pending = _readOne(); await pending;`）：
/// await 直接作用于调用表达式（`await _readOne()`）的协程 lowering 有
/// SSA 支配关系缺口；await 局部变量为已验证形态（与 CoreChannelReader
/// 的挂起读同形）。
/// </summary>
/// <typeparam name="T">元素类型。</typeparam>
internal class ChannelAllEnumerator<T> : IAsyncEnumerator<T> {
    private Func<Task<T>> _readOne;
    private T _current;

    /// <summary>以读取委托创建游标。</summary>
    /// <param name="readOne">单次读取委托。</param>
    public ChannelAllEnumerator(Func<Task<T>> readOne) {
        _readOne = readOne;
    }

    /// <summary>当前元素。</summary>
    public T Current { get { return _current; } }

    /// <summary>驱动到下一个元素：读到即缓存；终结排尽返回 false。</summary>
    /// <returns>前进成功返回 true；序列结束返回 false。</returns>
    public async Task<bool> MoveNextAsync() {
        try {
            Task<T> pending = _readOne();
            _current = await pending;
            return true;
        } catch (ChannelClosedException) {
            return false;
        }
    }
}
