// RFC 026 S5: TlsServerSession — TLS 1.3 服务端会话（公开服务器面）。
//
// `rt_crypto_tls_server_new`（S0 M4 测试 harness 专用）经 S5 提升为公开服务器面：
// 内存 BIO 非阻塞握手（同客户端栈复用）+ ALPN 协商 + 加密字节流读写 + 会话票证 +
// 客户端证书双向认证 + 0-RTT 早数据接收。归 Arc.Net.Security（RFC 025 P0
// 归属裁决：TLS 会话层归 Net，对齐客户端形态）。
// 底层 = vendored crypto_native.dll 的 `rt_crypto_tls_*` ABI（mbedTLS 4.1.1）。
//
// 配置（握手前）：`EnableSessionTickets`（会话恢复前置，默认开）/
// `ClientCertificateAuthority`（双向认证：验证客户端证书的信任锚）/
// `EnableEarlyData`（0-RTT 接收）。
//
// 诚实边界：OCSP stapling 后置；服务端早数据仅票证允许时接收（mbedTLS 4.x 实测限制
// 见 RFC 026 S5 注记）；`Read`/`Write` 同步语义保留。

namespace Arc.Net.Security;

using Arc.Collections;
using Arc.Text;
using Arc.Net;
using Arc.Security.Cryptography;
using Arc.Threading;

/// <summary>
/// TLS 1.3 服务端会话——非阻塞握手 + ALPN 协商 + 加密字节流读写。
/// 使用示例（e2e 形态）：
///   var server = new TcpListener(4433);
///   server.Start();
///   var sock = server.AcceptTcpClient();
///   var tls = new TlsServerSession(new NetworkStream(sock));
///   tls.EnableSessionTickets(true);
///   await tls.AuthenticateAsServerAsync(serverCert, serverKey);
///   int n = tls.Read(buf, 0, buf.Length);
///   tls.Write(reply, 0, reply.Length);
/// </summary>
public class TlsServerSession : IDisposable {
    private long _handle;               // opaque rt_crypto_tls_* 会话句柄
    private NetworkStream _stream;
    private List<string> _appProtocols;
    private bool _authenticated;
    private string _negotiated;

    // ── S5 服务端配置（握手前设置）──
    private bool _ticketsEnabled;       // flags 0x1：会话票证（默认 true）
    private X509Certificate2 _clientCa; // flags 0x2：客户端证书 VERIFY_REQUIRED 的信任锚
    private bool _verifyClient;
    private bool _earlyDataEnabled;     // flags 0x4：0-RTT 早数据接收

    /// <summary>构造 TLS 服务端会话。底层传输 = Arc.Net.NetworkStream（byte[] 面 · N3）。</summary>
    public TlsServerSession(NetworkStream inner) {
        if (inner == null) {
            throw new ArgumentNullException("inner");
        }
        _stream = inner;
        _appProtocols = new List<string>();
        _appProtocols.Add("h2");
        _appProtocols.Add("http/1.1");
        _ticketsEnabled = true;
        _negotiated = "";
    }

    /// <summary>ALPN 协议列表（握手前设置；默认 ["h2","http/1.1"]；空 = 不协商）。</summary>
    public List<string> ApplicationProtocols {
        get { return _appProtocols; }
        set { _appProtocols = value; }
    }

    /// <summary>会话票证启用（握手前设置；默认 true；关闭则客户端无法恢复会话）。</summary>
    public bool EnableSessionTickets {
        get { return _ticketsEnabled; }
        set { _ticketsEnabled = value; }
    }

    /// <summary>客户端证书双向认证：设置即要求客户端证书（VERIFY_REQUIRED）。
    /// 证书为验证客户端证书链的信任锚（DER；须含签发客户端证书的 CA）。</summary>
    public X509Certificate2 ClientCertificateAuthority {
        get { return _clientCa; }
        set { _clientCa = value; _verifyClient = (value != null); }
    }

    /// <summary>0-RTT 早数据接收启用（握手前设置；须票证启用）。</summary>
    public bool EnableEarlyData {
        get { return _earlyDataEnabled; }
        set { _earlyDataEnabled = value; }
    }

    /// <summary>ALPN 协商结果（"h2"/"http/1.1"/""）。握手完成后有效。</summary>
    public string NegotiatedApplicationProtocol {
        get { return _negotiated; }
    }

    /// <summary>是否已完成 TLS 1.3 全握手。</summary>
    public bool IsAuthenticated {
        get { return _authenticated; }
    }

    // ── 私有 [Builtin] ABI 直射（codegen 拦截；body 不执行）──

    /// <summary>创建 TLS 1.3 服务端会话 → opaque 句柄；失败返回 0。
    /// flags：0x1 = tickets；0x2 = 客户端证书 VERIFY_REQUIRED；0x4 = 早数据。</summary>
    [Builtin(ABI = "rt_crypto_tls_server_new_ex")]
    private static long _ServerNewEx(byte[] certDer, byte[] keyDer, byte[] alpnBlob,
                                     int flags, byte[] clientCaBlob) { return 0; }

    /// <summary>非阻塞握手一步：喂入 recv → send_out byte[]；state（0 = 等输入 / 1 = 完成）。</summary>
    [Builtin(ABI = "rt_crypto_tls_handshake")]
    private byte[] _Handshake(byte[] recv, out int state) { return null; }

    /// <summary>明文写 → 密文字节（内部 0x00 不截断）。</summary>
    [Builtin(ABI = "rt_crypto_tls_write")]
    private byte[] _Write(byte[] plain) { return null; }

    /// <summary>密文读 → 明文写入 buffer[offset..]；返回字节数（0 = EOF；-2 = 需更多输入）。</summary>
    [Builtin(ABI = "rt_crypto_tls_read")]
    private int _Read(byte[] enc, byte[] buffer, int offset, int count) { return 0; }

    /// <summary>协商出的 ALPN 协议（空串 = 未协商）。</summary>
    [Builtin(ABI = "rt_crypto_tls_alpn")]
    private string _Alpn() { return ""; }

    /// <summary>取走输出 FIFO 全部字节（flush post-handshake 消息如 NewSessionTicket）。</summary>
    [Builtin(ABI = "rt_crypto_tls_drain")]
    private byte[] _Drain() { return null; }

    /// <summary>早数据读（0-RTT 接收）：握手期吸收的早数据 → buffer[offset..]；返回字节数。</summary>
    [Builtin(ABI = "rt_crypto_tls_read_early_data")]
    private int _ReadEarlyData(byte[] enc, byte[] buffer, int offset, int count) { return 0; }

    /// <summary>释放会话句柄。</summary>
    [Builtin(ABI = "rt_crypto_tls_free")]
    private void _Free() {}

    // ── 异步握手（内存 BIO · 对齐客户端会话形态）──

    /// <summary>TLS 1.3 全握手（服务端；证书/私钥 DER 输入 · 非阻塞 · 底层同步传输）。
    /// 握手工作直接在本 async 方法内执行（对齐客户端 facade；Task.Run 委托包装下异常
    /// 无法经 C trampoline 展开——语言缺口，见 S5 验收注记）。失败抛异常。</summary>
    public async Task AuthenticateAsServerAsync(X509Certificate2 serverCertificate, byte[] privateKey) {
        this.DoAuthenticate(serverCertificate, privateKey);
    }

    /// <summary>TLS 1.3 全握手（服务端 · 同步；失败抛异常）。</summary>
    public void Authenticate(X509Certificate2 serverCertificate, byte[] privateKey) {
        this.DoAuthenticate(serverCertificate, privateKey);
    }

    private void DoAuthenticate(X509Certificate2 serverCertificate, byte[] privateKey) {
        if (_authenticated) {
            return;
        }
        if (serverCertificate == null) {
            throw new ArgumentNullException("serverCertificate");
        }
        if (privateKey == null || privateKey.Length == 0) {
            throw new ArgumentException("TlsServerSession requires a private key.");
        }
        byte[] certDer = serverCertificate.RawData;
        if (certDer == null || certDer.Length == 0) {
            throw new ArgumentException("TlsServerSession requires a DER certificate (CreateFromDer).");
        }
        byte[] alpnBlob = this.BuildAlpnBlob(_appProtocols);
        bool hasClientCa = _verifyClient && _clientCa != null && _clientCa.RawData != null;
        int flags = (_ticketsEnabled ? 0x1 : 0)
            + (hasClientCa ? 0x2 : 0)
            + (_earlyDataEnabled ? 0x4 : 0);
        byte[] caBlob = hasClientCa ? _clientCa.RawData : ZeroBytes(0);
        long h = _ServerNewEx(certDer, privateKey, alpnBlob, flags, caBlob);
        if (h == 0) {
            throw new InvalidOperationException("TlsServerSession: failed to create TLS session.");
        }
        _handle = h;

        TcpClient cl = _stream.BaseClient;
        int state = 0;
        byte[] recv = ZeroBytes(0);
        while (state != 1) {
            byte[] sendOut = _Handshake(recv, out state);
            if (sendOut == null) {
                throw new IOException("TLS 1.3 server handshake failed.");
            }
            if (sendOut.Length > 0) {
                int sent = cl.SendBytes(sendOut, 0, sendOut.Length);
                if (sent != sendOut.Length) {
                    throw new IOException("TLS 1.3 server handshake output send failed.");
                }
            }
            if (state == 1) {
                break;
            }
            byte[] buf = ZeroBytes(4096);
            int n = cl.ReceiveBytes(buf, 0, 4096);
            if (n == 0) {
                throw new IOException("TLS 1.3 server handshake input EOF.");
            }
            if (n < 0) {
                // 非阻塞传输无数据就绪：让出后重试握手循环（recv 已消费，置空防重复喂入）。
                Thread.Sleep(1);
                recv = ZeroBytes(0);
                continue;
            }
            byte[] next = ZeroBytes(n);
            for (int i = 0; i < n; i++) {
                next[i] = buf[i];
            }
            recv = next;
        }

        // flush 握手期生成的 post-handshake 消息（NewSessionTicket）。
        byte[] ticket = _Drain();
        if (ticket != null && ticket.Length > 0) {
            int sent = cl.SendBytes(ticket, 0, ticket.Length);
            if (sent != ticket.Length) {
                throw new IOException("TLS 1.3 server ticket send failed.");
            }
        }
        _authenticated = true;
        _negotiated = _Alpn();
    }

    // ── 加密字节流读写（语义对齐 NetworkStream.Read / Write）──

    /// <summary>解密明文读（byte[] 面）。返回实际字节数；EOF（close_notify）返回 0。</summary>
    public int Read(byte[] buffer, int offset, int count) {
        if (!_authenticated) {
            throw new InvalidOperationException("TlsServerSession is not authenticated.");
        }
        if (buffer == null) {
            throw new ArgumentNullException("buffer");
        }
        if (offset < 0 || count < 0 || offset + count > buffer.Length) {
            throw new ArgumentOutOfRangeException("offset/count");
        }
        if (count == 0) {
            return 0;
        }
        TcpClient cl = _stream.BaseClient;
        int attempts = 0;
        while (true) {
            attempts = attempts + 1;
            if (attempts > 4096) {
                throw new IOException("TLS read: too many WANT_READ iterations.");
            }
            byte[] empty = ZeroBytes(0);
            int n = _Read(empty, buffer, offset, count);
            if (n == -2) {
                byte[] buf = ZeroBytes(4096);
                int r = cl.ReceiveBytes(buf, 0, 4096);
                if (r == 0) {
                    return 0;
                }
                if (r < 0) {
                    // 非阻塞传输无数据就绪：短暂让出后重试 WANT_READ。
                    Thread.Sleep(1);
                    continue;
                }
                byte[] enc = ZeroBytes(r);
                for (int i = 0; i < r; i++) {
                    enc[i] = buf[i];
                }
                n = _Read(enc, buffer, offset, count);
                if (n == -2) {
                    // 密文可能仅含 post-handshake 消息（NewSessionTicket 等），
                    // mbedTLS 消费后以 WANT_READ 交还；空读排空排队数据后再回落 transport。
                    for (int drain = 0; drain < 4 && n == -2; drain++) {
                        n = _Read(empty, buffer, offset, count);
                    }
                    if (n == -2) {
                        continue;
                    }
                }
                if (n == 0) {
                    return 0;
                }
                if (n < 0) {
                    throw new IOException("TLS read failed.");
                }
                return n;
            }
            if (n == 0) {
                return 0;
            }
            if (n < 0) {
                throw new IOException("TLS read failed.");
            }
            return n;
        }
    }

    /// <summary>明文写 → 加密发送（全量；失败抛 IOException）。</summary>
    public void Write(byte[] buffer, int offset, int count) {
        if (!_authenticated) {
            throw new InvalidOperationException("TlsServerSession is not authenticated.");
        }
        if (buffer == null) {
            throw new ArgumentNullException("buffer");
        }
        if (offset < 0 || count < 0 || offset + count > buffer.Length) {
            throw new ArgumentOutOfRangeException("offset/count");
        }
        if (count == 0) {
            return;
        }
        byte[] plain = ZeroBytes(count);
        for (int i = 0; i < count; i++) {
            plain[i] = buffer[offset + i];
        }
        byte[] enc = _Write(plain);
        if (enc == null) {
            throw new IOException("TLS write failed.");
        }
        if (enc.Length == 0) {
            throw new IOException("TLS write requires inbound data (WANT_READ); retry after Read.");
        }
        TcpClient cl = _stream.BaseClient;
        int sent = cl.SendBytes(enc, 0, enc.Length);
        if (sent != enc.Length) {
            throw new IOException("TLS write send failed.");
        }
    }

    /// <summary>早数据读（0-RTT 接收；握手完成后调用）。返回早数据字节数；0 = 无更多早数据。</summary>
    public int ReadEarlyData(byte[] buffer, int offset, int count) {
        if (!_authenticated) {
            throw new InvalidOperationException("TlsServerSession is not authenticated.");
        }
        if (buffer == null) {
            throw new ArgumentNullException("buffer");
        }
        if (offset < 0 || count < 0 || offset + count > buffer.Length) {
            throw new ArgumentOutOfRangeException("offset/count");
        }
        byte[] empty = ZeroBytes(0);
        int n = _ReadEarlyData(empty, buffer, offset, count);
        if (n < 0) {
            throw new IOException("TLS early data read failed.");
        }
        return n;
    }

    /// <summary>释放 TLS 会话句柄与底层传输。</summary>
    public void Dispose() {
        if (_handle != 0) {
            _Free();
            _handle = 0;
        }
        if (_stream != null) {
            _stream.Close();
        }
    }

    // ── 私有工具 ──

    /// <summary>n 字节零填充数组（语言禁 `new T[expr]` 动态尺寸；同 Http2ByteUtils 惯例）。</summary>
    private static byte[] ZeroBytes(int n) {
        List<byte> buf = new List<byte>();
        int i = 0;
        while (i < n) {
            buf.Add((byte)0);
            i = i + 1;
        }
        return buf.ToArray();
    }

    /// <summary>ALPN 列表 → NUL 分隔 byte[]（C ABI 形态；空 = 不协商）。</summary>
    private byte[] BuildAlpnBlob(List<string> protocols) {
        List<byte> blob = new List<byte>();
        if (protocols != null) {
            for (int i = 0; i < protocols.Count; i++) {
                string p = protocols[i];
                if (p == null || p == "") {
                    continue;
                }
                byte[] pb = Encoding.GetBytes(p);
                for (int j = 0; j < pb.Length; j++) {
                    blob.Add(pb[j]);
                }
                blob.Add((byte)0);
            }
        }
        return blob.ToArray();
    }
}
