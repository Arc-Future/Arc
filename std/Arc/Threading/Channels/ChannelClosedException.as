// ChannelClosedException — 通道终结后读写异常（RFC 046）。
namespace Arc.Threading.Channels;

/// <summary>
/// 通道已终结后继续读写时抛出的异常。
/// 读端：缓冲排尽后 ReadAsync（或终结时无 error 的挂起读端）；
/// 写端：终结后 WriteAsync / 重复 Complete / 挂起写端被终结唤醒。
/// 终结携带的 error 原样传递给挂起读端，不经本异常包装。
/// </summary>
public class ChannelClosedException : Exception {
    /// <summary>以默认消息构造异常。</summary>
    public ChannelClosedException() : base("The channel has been closed.") {
    }

    /// <summary>以指定消息构造异常。</summary>
    /// <param name="message">错误描述。</param>
    public ChannelClosedException(string message) : base(message) {
    }
}
