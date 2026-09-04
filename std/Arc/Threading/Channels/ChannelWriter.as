// ChannelWriter<T> — 通道写端契约（RFC 046）。
namespace Arc.Threading.Channels;

using Arc;

/// <summary>
/// 通道写端契约——TryWrite 同步快路径 + WriteAsync 背压写 + Complete 终结。
/// 经 Channel&lt;T&gt;.Writer 获取。
/// </summary>
/// <typeparam name="T">元素类型。</typeparam>
public abstract class ChannelWriter<T> {
    /// <summary>同步写：有空位即入队（读等待者存在时直付）；满按 FullMode；终结后 false。</summary>
    /// <param name="item">元素值。</param>
    /// <returns>true 表示通道已接收（drop 模式下可能已按策略丢弃）。</returns>
    public abstract bool TryWrite(T item);

    /// <summary>
    /// 异步写：Wait 模式且满时挂起直至消费者腾出空位（真背压）；
    /// drop 模式即时按策略处理；终结后抛 ChannelClosedException。
    /// </summary>
    /// <param name="item">元素值。</param>
    /// <param name="cancellationToken">取消令牌。</param>
    public abstract Task WriteAsync(T item, CancellationToken cancellationToken = default);

    /// <summary>
    /// 终结通道：挂起读端以 error（无 error 则 ChannelClosedException）失败、
    /// 挂起写端以 ChannelClosedException 失败；缓冲中已产出值仍可消费完毕；
    /// 排尽后读端 Completion 完成。重复终结抛 ChannelClosedException。
    /// </summary>
    /// <param name="error">终结原因（null 表示正常终结）。</param>
    public abstract void Complete(Exception? error = null);
}
