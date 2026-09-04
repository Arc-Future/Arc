// CoreChannelWriter<T> — 通道写端实现（RFC 046）。
namespace Arc.Threading.Channels;

using Arc;

/// <summary>
/// ChannelWriter&lt;T&gt; 的共用实现——有界/无界通道共享（核心在 ChannelCore）。
/// </summary>
/// <typeparam name="T">元素类型。</typeparam>
internal class CoreChannelWriter<T> : ChannelWriter<T> {
    private ChannelCore<T> _core;

    /// <summary>绑定核心构造写端。</summary>
    /// <param name="core">通道状态机核心。</param>
    public CoreChannelWriter(ChannelCore<T> core) {
        _core = core;
    }

    /// <summary>同步写：有空位即入队（读等待者存在时直付）；满按 FullMode；终结后 false。</summary>
    /// <param name="item">元素值。</param>
    /// <returns>true 表示通道已接收（drop 模式下可能已按策略丢弃）。</returns>
    public override bool TryWrite(T item) {
        return _core.TryWrite(item);
    }

    /// <summary>
    /// 异步写：Wait 模式且满时挂起写等待者（FIFO），消费者腾出空位后元素
    /// 收纳入缓冲并唤醒（O(1) 交接，无竞态重试环）；终结后抛
    /// ChannelClosedException；取消经 ct 协作中断（挂起中的元素不会被写入）。
    /// </summary>
    /// <param name="item">元素值。</param>
    /// <param name="cancellationToken">取消令牌。</param>
    public override async Task WriteAsync(T item, CancellationToken cancellationToken = default) {
        CancellationToken ct = cancellationToken;
        ct.ThrowIfCancellationRequested();
        if (_core.TryWrite(item)) {
            return;
        }
        ChannelWriterWaiter<T> waiter = new ChannelWriterWaiter<T>(item);
        Task<bool> pending = _core.WriteEnqueue(waiter);
        // None 令牌不注册取消回调（rt_ct_register 对默认令牌无 cts 载体）。
        if (ct.CanBeCanceled) {
            ct.Register(() => _core.CancelWrite(waiter));
        }
        if (ct.IsCancellationRequested) {
            _core.CancelWrite(waiter);
        }
        await pending;
        if (waiter.Served) {
            return;
        }
        ct.ThrowIfCancellationRequested();
    }

    /// <summary>终结通道（语义见契约）。</summary>
    /// <param name="error">终结原因（null 表示正常终结）。</param>
    public override void Complete(Exception? error = null) {
        _core.Complete(error);
    }
}
