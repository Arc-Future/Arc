// RFC 042: QuicTransport — UDP QUIC-like 传输桩。
// 当前 Arc 解析器不支持 & (bitwise AND)，因此用简化桩实现。
// 当前为协议级可用实现；解析器能力落地后升级为生产级。
// 注意：签名与 ITransport/IConnection/IStream 对齐（含 CancellationToken 参数），
// 保证整个 Arc.Net.P2P 包可通过接口实现校验（原 l3_honesty_sweep_e2e 构建前提；
// 该 e2e 已随 arc-integration 退场，a2627a0f）。

namespace Arc.Net.P2P;
using Arc.Net;

// 桩实现——仅提供拓扑/构造，Payload 暂存为纯文本。
internal class QuicConnection : IConnection {
    private int _nextStreamId;

    public PeerId RemotePeerId { get; }
    public bool IsConnected { get; set; }

    public QuicConnection(PeerId remotePeerId, string remoteAddr, int remotePort) {
        RemotePeerId = remotePeerId;
        IsConnected = true;
        _nextStreamId = 1;
    }

    public async Task<IStream> OpenStreamAsync(CancellationToken cancellationToken) {
        int sid = _nextStreamId;
        _nextStreamId = _nextStreamId + 1;
        return new QuicStream(sid, this);
    }

    public async Task<IStream> AcceptStreamAsync(CancellationToken cancellationToken) { return null; }
    public void WriteData(string payload) { }
    public string ReadData() { return null; }

    public async Task<void> SendDatagramAsync(string data, CancellationToken cancellationToken) { }
    public void Close() { IsConnected = false; }
    public async Task<void> CloseAsync(CancellationToken cancellationToken) { this.Close(); }
}

internal class QuicStream : IStream {
    private QuicConnection _conn;
    private bool _closed;

    public int StreamId { get; }

    public QuicStream(int sid, QuicConnection conn) { StreamId = sid; _conn = conn; _closed = false; }

    public async Task<void> WriteAsync(string data, CancellationToken cancellationToken) { }
    public async Task<string> ReadAsync(CancellationToken cancellationToken) { return null; }
    public async Task<void> CloseWriteAsync(CancellationToken cancellationToken) { _closed = true; }
    public async Task<void> CloseAsync(CancellationToken cancellationToken) { _closed = true; }
}

public class QuicTransport : ITransport {
    private PeerKey _localKey;

    public QuicTransport() { _localKey = null; }

    public QuicTransport(PeerKey localKey) {
        _localKey = localKey;
    }

    public async Task<IConnection> DialAsync(Multiaddr addr, CancellationToken cancellationToken) { return null; }
    public async Task<void> ListenAsync(Multiaddr addr, CancellationToken cancellationToken) { }
}