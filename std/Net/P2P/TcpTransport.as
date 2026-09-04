// RFC 042 M1 (M1a): TcpTransport — 连接/传输生产级恢复。
//
// 真实面（对齐 RFC 042 M1 + libp2p transport 语义）：
//   - TCP 连接真实：`Arc.Net.TcpClient` → `rt_socket_connect`（std/Net/Core 同步 socket 面）。
//   - 连接对象**保留 live socket**：`TcpClient` 为 opaque handle（`[Builtin(rt_socket_create)]`）
//     + byte[]/async 面；本对象持有 `TcpClient` + `NetworkStream` 跨方法/跨 await 使用
//     （对齐 `Http2Connection` 既有先例；旧 RFC 042 的「连接期 socket 不保留」缺口已解除）。
//   - 协商真实：拨号/接受后经 `MultistreamSelect`（multistream-select/1.0.0）握手 + 协议选择。
//   - 协议注册：`RegisterProtocol` 登记监听器可接受的协议标识，接受侧据此协商。
//
// 诚实边界（M1a/M1b 切片；对齐 RFC 042 M1 触碰面）：
//   - M1a 完成「TCP 完整（ListenAsync + 连接生命周期 + 协议注册）+ multistream-select 协商」。
//   - **M1b 完成 yamux 流复用**：TcpConnection.OpenStream/AcceptStream → YamuxSession
//     （std/Net/P2P/Yamux.as；单连接多逻辑流 + 窗口 + 后台 reader 解复用）。
//   - **Noise XX 全流程属 M2**（本切片不宣称安全握手闭环；协商协议标识由调用方/测试给定）。
//   - 帧长度 varint 单字节（协议标识 <128；见 MultistreamSelect.as 诚实边界）。
//   - 交互式协商（`ls` 列出）后置；本切片仅"选择单一协议"，多协议/列表后续扩展。
namespace Arc.Net.P2P;

using Arc;
using Arc.Net;
using Arc.Collections;

public class TcpTransport : ITransport {
    private List<string> _supported;
    private TcpListener _listener;

    public TcpTransport() {
        _supported = new List<string>();
        _listener = null;
    }

    // ── 协议注册 ──

    /// <summary>登记监听器可接受的协议标识（接受侧 multistream-select 据此协商）。</summary>
    public void RegisterProtocol(string protocolId) {
        if (protocolId == null || protocolId == "") {
            throw new ArgumentException("TcpTransport.RegisterProtocol: empty protocol id");
        }
        _supported.Add(protocolId);
    }

    // ── 客户端拨号（生产级） ──

    /// <summary>
    /// 客户端拨号（同步、typechecked）：Multiaddr 解析 → TCP 连接 → multistream-select
    /// 客户端握手 → 选择 <paramref name="protocolId"/> → TcpConnection（保留 live socket）。
    /// 失败抛 IOException。具体类型返回，规避接口胖指针 vtable 缺口（见文件头）。
    /// </summary>
    public TcpConnection Dial(Multiaddr addr, string protocolId) {
        string host = addr.GetValue(MultiaddrProtocol.IP4);
        string portStr = addr.GetValue(MultiaddrProtocol.Tcp);
        if (host == "" || portStr == "") {
            throw new ArgumentException("TcpTransport.Dial: multiaddr requires /ip4/<host>/tcp/<port>");
        }
        int port = Convert.ToInt32(portStr);

        TcpClient client = new TcpClient();
        bool connected = client.Connect(host, port);
        if (!connected) {
            client.Close();
            throw new IOException("TcpTransport.Dial: TCP connect failed to " + host + ":" + portStr);
        }

        NetworkStream stream = new NetworkStream(client);
        // multistream-select 客户端握手。
        bool handshakeOk = MultistreamSelect.ClientHandshake(stream);
        if (!handshakeOk) {
            client.Close();
            throw new IOException("TcpTransport.Dial: multistream-select handshake failed");
        }
        // 选择协议；可选（protocolId 为空时跳过选择，仅保留握手）。
        string negotiated = protocolId;
        if (protocolId != null && protocolId != "") {
            string sel = MultistreamSelect.ClientSelect(stream, protocolId);
            if (sel == null || sel == "na") {
                client.Close();
                throw new IOException("TcpTransport.Dial: protocol not negotiated: " + protocolId);
            }
            negotiated = sel;
        }

        return new TcpConnection(client, stream, new PeerId(addr.ToString()), negotiated, false);
    }

    /// <summary>拨号（默认无协议选择，仅 TCP + multistream 握手）。</summary>
    public TcpConnection Dial(Multiaddr addr) {
        return this.Dial(addr, null);
    }

    public async Task<IConnection> DialAsync(Multiaddr addr, CancellationToken cancellationToken) {
        // Task<IConnection> 跨方法返回时接口胖指针 vtable 不完整（编译器缺口）——
        // 调用方先 await 到局部再单独转具体类型走具体分派（见文件头）。
        return (IConnection)this.Dial(addr);
    }

    // ── 服务端监听/接受（生产级） ──

    /// <summary>绑定本地端口并开始监听。成功返回 true。</summary>
    public bool Listen(Multiaddr addr) {
        string portStr = addr.GetValue(MultiaddrProtocol.Tcp);
        if (portStr == "") {
            throw new ArgumentException("TcpTransport.Listen: multiaddr requires /tcp/<port>");
        }
        int port = Convert.ToInt32(portStr);
        _listener = new TcpListener();
        return _listener.Start(port);
    }

    public async Task<void> ListenAsync(Multiaddr addr, CancellationToken cancellationToken) {
        this.Listen(addr);
    }

    /// <summary>
    /// 接受一个传入连接并完成 multistream-select 服务端协商（握手 + 协议选择）。
    /// 阻塞直到有连接到达；返回协商后的 TcpConnection（保留 live socket）。
    /// </summary>
    public TcpConnection Accept() {
        if (_listener == null) {
            throw new IOException("TcpTransport.Accept: no active listener (call Listen first)");
        }
        TcpClient client = _listener.AcceptTcpClient();
        if (client == null) {
            throw new IOException("TcpTransport.Accept: accept failed");
        }
        NetworkStream stream = new NetworkStream(client);
        bool handshakeOk = MultistreamSelect.ServerHandshake(stream);
        if (!handshakeOk) {
            client.Close();
            throw new IOException("TcpTransport.Accept: multistream-select server handshake failed");
        }
        string negotiated = MultistreamSelect.ServerHandle(stream, _supported);
        return new TcpConnection(client, stream, new PeerId("listener"), negotiated, true);
    }

    /// <summary>停止监听并释放资源。</summary>
    public void Stop() {
        if (_listener != null) {
            _listener.Stop();
            _listener = null;
        }
    }
}

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
