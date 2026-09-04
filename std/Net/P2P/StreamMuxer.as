// RFC 042 M1 (M1b): StreamMuxer — 流复用器门面（真实实现，委托 YamuxSession）。
//
// 真实流复用见 Yamux.as（yamux/1.0.0 会话：帧收发 + 流表 + 窗口 + 后台 reader）。
// 本类为高层门面（RFC 042 抽象）：持有底层字节连接，提供 OpenStream/AcceptStream/Close。
// 对齐 libp2p muxer 语义：单连接承载多逻辑流；streamID 按角色（拨号/监听）奇偶分配。
namespace Arc.Net.P2P;

using Arc.Net;

internal class StreamMuxer {
    private YamuxSession _session;

    /// <param name="client">已建立连接的 TcpClient（二进制安全 byte[] 面）。</param>
    /// <param name="isServer">true=监听方（偶数 streamID），false=拨号方（奇数）。</param>
    public StreamMuxer(TcpClient client, bool isServer) {
        _session = new YamuxSession(client, isServer);
    }

    /// <summary>开启一条逻辑流（阻塞获取；发送 WindowUpdate+SYN 开流）。</summary>
    public YamuxStream OpenStream() {
        return _session.OpenStream();
    }

    /// <summary>接受一条入站逻辑流（阻塞直到有流或会话关闭）。</summary>
    public YamuxStream AcceptStream() {
        return _session.AcceptStream();
    }

    /// <summary>关闭会话（关底层连接并唤醒所有等待）。</summary>
    public void Close() {
        _session.Close();
    }
}
