// CoreChannelReader<T> — 通道读端实现（RFC 046）。
namespace Arc.Threading.Channels;

using Arc;

/// <summary>
/// ChannelReader&lt;T&gt; 的共用实现——有界/无界通道共享（核心在 ChannelCore）。
/// </summary>
/// <typeparam name="T">元素类型。</typeparam>
internal class CoreChannelReader<T> : ChannelReader<T> {
    private ChannelCore<T> _core;

    /// <summary>绑定核心构造读端。</summary>
    /// <param name="core">通道状态机核心。</param>
    public CoreChannelReader(ChannelCore<T> core) {
        _core = core;
    }

    /// <summary>是否支持计数（当前实现恒 true）。</summary>
    public override bool CanCount() {
        return true;
    }

    /// <summary>缓冲中当前元素数（直付中的元素不计入）。</summary>
    public override int Count() {
        return _core.Count;
    }

    /// <summary>完成信号 Task：终结且排尽后正常完成（true）；以 error 终结则携带 error 失败。</summary>
    public override Task<bool> Completion() {
        return _core.CompletionGate;
    }

    /// <summary>同步读：缓冲有值出队；空（含终结排尽）返回 false。</summary>
    /// <param name="item">出队元素。</param>
    /// <returns>true 表示取得元素。</returns>
    public override bool TryRead(out T item) {
        item = default(T);
        bool got = _core.TryRead(out item);
        return got;
    }

    /// <summary>
    /// 异步读：快路径同步完成；空载挂起读等待者直至元素直付或终结；
    /// 取消经 ct 协作中断（登记-注册窗口由注册后复查闭合；唤醒后按等待者
    /// 终态分派：已交付返回 / 终结复查重查缓冲余量 / 已取消抛 OCE；
    /// 复查后缓冲仍空经 ReadEnqueue 抛出终结异常）。
    /// </summary>
    /// <param name="cancellationToken">取消令牌。</param>
    /// <returns>读取的元素。</returns>
    public override async Task<T> ReadAsync(CancellationToken cancellationToken = default) {
        CancellationToken ct = cancellationToken;
        ct.ThrowIfCancellationRequested();
        while (true) {
            T item = default(T);
            if (_core.TryRead(out item)) {
                return item;
            }
            ChannelReaderWaiter<T>? waiter = null;
            Task<T> pending = _core.ReadEnqueue(out waiter);
            if (waiter != null) {
                // None 令牌不注册取消回调（rt_ct_register 对默认令牌无 cts 载体）。
                if (ct.CanBeCanceled) {
                    ct.Register(() => _core.CancelRead(waiter));
                }
                if (ct.IsCancellationRequested) {
                    _core.CancelRead(waiter);
                }
            }
            T value = await pending;
            if (waiter == null || waiter.Served) {
                return value;
            }
            if (waiter.Recheck) {
                continue;
            }
            ct.ThrowIfCancellationRequested();
        }
    }
}
