// MultiaddrProtocol —— 拆分自 Multiaddr.as（一文件一公开类型）。
namespace Arc.Net.P2P;

public enum MultiaddrProtocol {
    IP4,
    IP6,
    Tcp,
    Udp,
    Dns,
    Dns4,
    Dns6,
    Quic,
    WS,
    Wss,
}
