// TcpConnection —— 拆分自 TcpTransport.as（一文件一公开类型）。
namespace Arc.Net.P2P;
using Arc;
using Arc.Net;
using Arc.Collections;

/// <summary>
/// 真实连接对象：持有 live `TcpClient` + `NetworkStream`（保留 socket 跨方法/await，
/// opaque handle 语义）＋ 协商协议标识。文本级收发委托 NetworkStream（M1a 原始流）；
/// 流复用经 YamuxSession（M1b：单连接多逻辑流，后台 reader 解复用）。
/// </summary>
public class TcpConnection : IConnection {
    private TcpClient _client;
    private bool _isServer;
    private YamuxSession _yamux;

    public PeerId RemotePeerId { get; }
    public bool IsConnected { get; set; }
    public string NegotiatedProtocol { get; }
    public NetworkStream Stream { get; }

    public TcpConnection(TcpClient client, NetworkStream stream, PeerId remotePeerId, string protocol, bool isServer) {
        _client = client;
        Stream = stream;
        RemotePeerId = remotePeerId;
        IsConnected = true;
        NegotiatedProtocol = protocol;
        _isServer = isServer;
        _yamux = null;
    }



    /// <summary>获取（惰性创建）本连接的 yamux 会话。</summary>
    private YamuxSession EnsureYamux() {
        if (_yamux == null) {
            _yamux = new YamuxSession(_client, _isServer);
        }
        return _yamux;
    }

    /// <summary>开启一条 yamux 逻辑流（M1b；返回具体类型规避接口胖指针 vtable 缺口）。</summary>
    public YamuxStream OpenStream() {
        return this.EnsureYamux().OpenStream();
    }

    /// <summary>接受一条 yamux 入站逻辑流（M1b；阻塞直到有流）。</summary>
    public YamuxStream AcceptStream() {
        return this.EnsureYamux().AcceptStream();
    }

    // ── 文本级收发（M1a 原始流 · 委托 NetworkStream） ──

    /// <summary>发送一行文本（追加 \n）。返回发送字节数。</summary>
    public int WriteLine(string data) {
        return Stream.WriteString(data + "\n");
    }

    /// <summary>读取一行（至 \n，剥离 \r）。EOF 返回 null。</summary>
    public string ReadLine() {
        return Stream.ReadLine();
    }

    /// <summary>发送原始字节。</summary>
    public void WriteBytes(byte[] data) {
        Stream.Write(data, 0, data.Length);
    }

    /// <summary>读取原始字节（部分读语义）。</summary>
    public int ReadBytes(byte[] buffer, int offset, int count) {
        return Stream.Read(buffer, offset, count);
    }

    // ── 接口实现（M1b：Open/AcceptStreamAsync 接入 Yamux） ──

    public async Task<IStream> OpenStreamAsync(CancellationToken cancellationToken) {
        return (IStream)this.OpenStream();
    }
    public async Task<IStream> AcceptStreamAsync(CancellationToken cancellationToken) {
        return (IStream)this.AcceptStream();
    }
    public async Task<void> SendDatagramAsync(string data, CancellationToken cancellationToken) {
        throw new NotImplementedException("TcpConnection.SendDatagramAsync not implemented (datagram = UDP surface).");
    }
    public async Task<void> CloseAsync(CancellationToken cancellationToken) {
        this.Close();
    }

    /// <summary>同步关闭。</summary>
    public void Close() {
        if (_yamux != null) {
            _yamux.Close();
            _yamux = null;
        }
        IsConnected = false;
        Stream.Close();
    }
}
