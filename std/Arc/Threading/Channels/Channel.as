// Channel<T> — 通道句柄（RFC 046）。
namespace Arc.Threading.Channels;

/// <summary>
/// 多生产者/多消费者通道句柄——读写端分离的异步通信原语。
/// 经 Channels 工厂创建（有界/无界）；构造函数收 internal 读写端实现，
/// 外部无法绕过工厂实例化。Reader/Writer 为构造期即定的只读面
///（自定义 getter 属性，构造后不变）。
/// </summary>
/// <typeparam name="T">元素类型。</typeparam>
public class Channel<T> {
    private ChannelReader<T> _reader;
    private ChannelWriter<T> _writer;

    /// <summary>读端契约（构造期即定，此后只读）。</summary>
    public ChannelReader<T> Reader { get { return _reader; } }

    /// <summary>写端契约（构造期即定，此后只读）。</summary>
    public ChannelWriter<T> Writer { get { return _writer; } }

    /// <summary>以读写端构造通道（工厂先物化读写端，构造期纯绑定）。</summary>
    /// <param name="reader">读端实现（工厂物化）。</param>
    /// <param name="writer">写端实现（工厂物化）。</param>
    internal Channel(ChannelReader<T> reader, ChannelWriter<T> writer) {
        _reader = reader;
        _writer = writer;
    }
}
