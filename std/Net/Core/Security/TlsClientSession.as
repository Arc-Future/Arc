// RFC 035 M3/M4 → S5: TlsClientSession — TLS 1.3 客户端会话（内存 BIO 非阻塞握手 + ALPN）。
//
// 对标 C# System.Net.Security.SslStream（去糟粕）：TLS 1.3 客户端会话面，归位裁决
// 见 RFC 035 §1.2 ⑤（唯一形态 = TlsClientSession）；RFC 025 P0 归属裁决：TLS
// 会话层归 Net（对标 System.Net.Security），namespace = Arc.Net.Security。
// 底层 = vendored crypto_native.dll 的 `rt_crypto_tls_*` ABI（mbedTLS 4.1.1）。
//
// 握手形态：内存 BIO 非阻塞（mbedtls_ssl_set_bio 双 FIFO）——`rt_crypto_tls_handshake`
// 喂入 recv、产出 send_out、返回 state（0 = 需更多输入 / 1 = 完成 / -1 = 出错）；
// `AuthenticateAsClientAsync` 内循环：调握手 → 有输出写传输 → 无输出则读下一块 → 直至完成。
// 传输字节面：TLS 记录为任意二进制（含内部 0x00），不得经 NetworkStream 的 string 面
// （N3 诚实边界：底层 NUL 终止原语截断）——经 `TcpClient.SendBytes/ReceiveBytes`
// （rt_socket_send / rt_net_recv 显式长度）闭环。
//
// S5（026 M3 L269 后置项完结，兼容演进）：
//   · 完整证书链校验——`VerifyMode` 显式校验策略（None/Anchor/FullChain）+ `TrustAnchors`
//     PEM 链 + `CrlData`（CRL 最小面）+ `VerifyResult`；`TrustAnchor` 语义从「null=不校验」
//     兼容演进为「未显式策略时的最小锚校验」（S3 wss 自签根测试面保持可用）。
//   · 会话恢复——`LoadSession(byte[])`（握手前）+ `SaveSession()`（握手后；票证含内部 0x00）。
//   · 0-RTT——`EarlyDataEnabled` + `EarlyDataPayload` + `EarlyDataStatus`
//     （0=未指示 / 1=ACCEPTED / 2=REJECTED；mbedTLS 4.x 实测限制见 RFC 035 S5 注记）。
//   · 双向认证——`ClientCertificate` + `ClientPrivateKey`（PKCS#8/PKCS#1 DER）。
//
// 诚实边界（S5 更新）：OCSP stapling 后置；0-RTT 仅 ticket 允许早数据时生效，
// 不接受时静默退正常握手（RFC 8446 §8.1 语义）；`Read`/`Write` 同步语义保留。

namespace Arc.Net.Security;

using Arc.Collections;
using Arc.Text;
using Arc.Net;
using Arc.Security.Cryptography;
using Arc.Threading;

/// <summary>TLS 证书校验策略（S5：`TrustAnchor` 语义从「null=不校验」升级为显式策略）。</summary>
public enum TlsCertificateVerification {
    /// <summary>不校验对端证书（仅测试面；显式设置时覆盖锚）。</summary>
    None,
    /// <summary>信任锚最小校验（单 DER 锚；等同 M3 行为）。</summary>
    Anchor,
    /// <summary>完整链校验（根+中间 PEM 链；含有效期/主机名/吊销 CRL 最小面）。</summary>
    FullChain
}

/// <summary>
/// TLS 1.3 客户端会话——非阻塞握手 + ALPN 协商 + 加密字节流读写 + 校验策略/会话恢复/0-RTT。
/// 使用示例（e2e 形态）：
///   var client = new TcpClient();
///   client.Connect("127.0.0.1", 4433);
///   var tls = new TlsClientSession(new NetworkStream(client));
///   tls.TargetHost = "localhost";
///   tls.TrustAnchor = X509Certificate2.CreateFromDer(trustDer);
///   await tls.AuthenticateAsClientAsync();
///   tls.Write(plain, 0, plain.Length);
///   int n = tls.Read(buf, 0, buf.Length);
/// </summary>
public class TlsClientSession : IDisposable {
    private long _handle;               // opaque rt_crypto_tls_* 会话句柄（codegen offset 16）
    private NetworkStream _stream;
    private string _targetHost;
    private List<string> _appProtocols;
    private X509Certificate2 _trustAnchor;
    private bool _authenticated;
    private string _negotiated;

    // ── S5 校验策略 / 会话恢复 / 0-RTT / 双向认证 状态 ──
    private int _verifyModeSet;                    // 0 = 未显式设置（按锚兼容演进）
    private TlsCertificateVerification _verifyMode;
    private List<X509Certificate2> _trustAnchors;  // FullChain 模式：根+中间 PEM 链
    private bool _useSystemRoots;                  // 默认 FullChain：无显式锚 → OS 根证书
    private byte[] _crlData;                       // DER CRL（吊销 · 最小面）
    private X509Certificate2 _clientCert;          // 双向认证客户端证书
    private byte[] _clientKey;                     // 客户端私钥（PKCS#8/PKCS#1 DER）
    private byte[] _sessionBytes;                  // 恢复会话（握手前载入）
    private byte[] _earlyDataPayload;              // 0-RTT 载荷（握手前设置）
    private bool _earlyDataEnabled;
    private int _earlyDataStatus;                  // 握手后：0 未指示 / 1 ACCEPTED / 2 REJECTED

    /// <summary>构造 TLS 客户端会话。底层传输 = Arc.Net.NetworkStream（byte[] 面 · N3）。</summary>
    public TlsClientSession(NetworkStream inner) {
        if (inner == null) {
            throw new ArgumentNullException("inner");
        }
        _stream = inner;
        _targetHost = "localhost";
        _appProtocols = new List<string>();
        _appProtocols.Add("h2");
        _appProtocols.Add("http/1.1");
        _trustAnchors = new List<X509Certificate2>();
        _useSystemRoots = true;
        _verifyMode = TlsCertificateVerification.None;
        _negotiated = "";
    }

    /// <summary>SNI + 证书校验主机名（握手前设置；默认 "localhost"）。</summary>
    public string TargetHost {
        get { return _targetHost; }
        set { _targetHost = value; }
    }

    /// <summary>ALPN 协议列表（握手前设置；默认 ["h2","http/1.1"]；空 = 不协商）。</summary>
    public List<string> ApplicationProtocols {
        get { return _appProtocols; }
        set { _appProtocols = value; }
    }

    /// <summary>信任锚（自签根，最小校验）；null = 不校验（仅测试面）。
    /// S5 兼容演进：未显式设置 <see cref="VerifyMode"/> 时，其存在与否决定最小校验策略。</summary>
    public X509Certificate2 TrustAnchor {
        get { return _trustAnchor; }
        set { _trustAnchor = value; }
    }

    /// <summary>显式校验策略（S5）：None / Anchor / FullChain。未设置时按锚兼容演进。</summary>
    public TlsCertificateVerification VerifyMode {
        get { return _verifyMode; }
        set { _verifyMode = value; _verifyModeSet = 1; }
    }

    /// <summary>完整链校验信任锚链（根+中间，PEM 或 DER 证书列表；FullChain 模式使用）。</summary>
    public List<X509Certificate2> TrustAnchors {
        get { return _trustAnchors; }
        set { _trustAnchors = value; }
    }

    /// <summary>默认 FullChain 校验时是否载入 OS 系统根证书（握手前设置；默认 true）。
    /// true = 未设显式信任锚时以系统根做完整链校验（真实公网主机可用）；
    /// false = 必须显式提供 <see cref="TrustAnchor"/>/<see cref="TrustAnchors"/> 或
    /// 显式设 <see cref="VerifyMode"/> = None，否则 fail-closed（无根可验不静默降级）。</summary>
    public bool UseSystemRoots {
        get { return _useSystemRoots; }
        set { _useSystemRoots = value; }
    }

    /// <summary>吊销 CRL（DER 编码；最小面校验；握手前设置）。</summary>
    public byte[] CrlData {
        get { return _crlData; }
        set { _crlData = value; }
    }

    /// <summary>双向认证客户端证书（握手前设置）。</summary>
    public X509Certificate2 ClientCertificate {
        get { return _clientCert; }
        set { _clientCert = value; }
    }

    /// <summary>双向认证客户端私钥（PKCS#8 或 PKCS#1 DER；握手前设置）。</summary>
    public byte[] ClientPrivateKey {
        get { return _clientKey; }
        set { _clientKey = value; }
    }

    /// <summary>0-RTT 早数据载荷（握手前设置；须 <see cref="EarlyDataEnabled"/> = true）。
    /// 握手循环内自动写出；ticket 不允许早数据时静默退正常握手（载荷不写出，诚实边界）。</summary>
    public byte[] EarlyDataPayload {
        get { return _earlyDataPayload; }
        set { _earlyDataPayload = value; }
    }

    /// <summary>0-RTT 早数据启用（握手前设置；须载入允许早数据的恢复会话）。</summary>
    public bool EarlyDataEnabled {
        get { return _earlyDataEnabled; }
        set { _earlyDataEnabled = value; }
    }

    /// <summary>早数据状态（握手完成后）：0 = 未指示 / 1 = ACCEPTED / 2 = REJECTED。</summary>
    public int EarlyDataStatus {
        get { return _earlyDataStatus; }
    }

    /// <summary>握手后校验结果位标志（0 = 通过；VERIFY_REQUIRED 时有效）。</summary>
    public int VerifyResult {
        get {
            if (!_authenticated) {
                return -1;
            }
            return this._VerifyResult();
        }
    }

    /// <summary>ALPN 协商结果（"h2"/"http/1.1"/""）。握手完成后有效。</summary>
    public string NegotiatedApplicationProtocol {
        get { return _negotiated; }
    }

    /// <summary>是否已完成 TLS 1.3 全握手。</summary>
    public bool IsAuthenticated {
        get { return _authenticated; }
    }

    /// <summary>保存当前会话（序列化字节；含内部 0x00，显式长度由 byte[] 承载）。
    /// 须在握手完成且已处理 NewSessionTicket 之后调用（首个 Read 往返即吸收票证）。</summary>
    public byte[] SaveSession() {
        if (!_authenticated) {
            throw new InvalidOperationException("TlsClientSession is not authenticated.");
        }
        return this._SessionSave();
    }

    /// <summary>载入恢复会话（握手前调用）。载入失败不中断——退全握手（mbedTLS 语义）。</summary>
    public void LoadSession(byte[] sessionBytes) {
        byte[] sb = sessionBytes;
        if (sb == null || sb.Length == 0) {
            throw new ArgumentException("TlsClientSession.LoadSession requires non-empty bytes.");
        }
        _sessionBytes = sessionBytes;
    }

    // ── 私有 [Builtin] ABI 直射（codegen 拦截；body 不执行）──

    /// <summary>创建 TLS 1.3 客户端会话 → opaque 句柄；失败返回 0。</summary>
    [Builtin(ABI = "rt_crypto_tls_client_new")]
    private static long _ClientNew(string serverName, byte[] trustDer, byte[] alpnBlob) { return 0; }

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

    /// <summary>释放会话句柄。</summary>
    [Builtin(ABI = "rt_crypto_tls_free")]
    private void _Free() {}

    // ── S5 ABI 直射 ──

    /// <summary>校验策略（mode 0/1/2）+ 信任链/锚 blob（DER 或 PEM）。返回 0 成功。</summary>
    [Builtin(ABI = "rt_crypto_tls_set_verify")]
    private int _SetVerify(int mode, byte[] trustBlob) { return 0; }

    /// <summary>载入 OS 系统根证书到会话 CA 链并置 VERIFY_REQUIRED（真实公网主机校验）。
    /// 与 <see cref="_SetVerify"/> 互斥（任择其一）。返回 0 成功 / 负值失败。</summary>
    [Builtin(ABI = "rt_crypto_tls_load_system_roots")]
    private int _LoadSystemRoots() { return 0; }

    /// <summary>载入 DER CRL（吊销最小面）。返回 0 成功。</summary>
    [Builtin(ABI = "rt_crypto_tls_set_crl")]
    private int _SetCrl(byte[] crlDer) { return 0; }

    /// <summary>握手后校验结果位标志（0 = 通过）。</summary>
    [Builtin(ABI = "rt_crypto_tls_verify_result")]
    private int _VerifyResult() { return 0; }

    /// <summary>双向认证：客户端证书 DER + 私钥 DER。返回 0 成功。</summary>
    [Builtin(ABI = "rt_crypto_tls_set_client_cert")]
    private int _SetClientCert(byte[] certDer, byte[] keyDer) { return 0; }

    /// <summary>会话序列化保存（byte[]；含内部 0x00）。</summary>
    [Builtin(ABI = "rt_crypto_tls_session_save")]
    private byte[] _SessionSave() { return null; }

    /// <summary>会话载入（握手前恢复）。返回 0 成功 / 负值失败。</summary>
    [Builtin(ABI = "rt_crypto_tls_session_load")]
    private int _SessionLoad(byte[] bytes) { return 0; }

    /// <summary>0-RTT 早数据启用（握手前）。返回 0 成功。</summary>
    [Builtin(ABI = "rt_crypto_tls_enable_early_data")]
    private int _EnableEarlyData(int enabled) { return 0; }

    /// <summary>0-RTT 早数据写（握手期间）：喂 recv + plain → 密文字节；
    /// state（1=写出 / 0=等输入 / -1=无法写·退正常握手 / -2=硬错误）。</summary>
    [Builtin(ABI = "rt_crypto_tls_write_early_data")]
    private byte[] _WriteEarlyData(byte[] recv, byte[] plain, out int state) { return null; }

    /// <summary>早数据状态（握手后）：0 未指示 / 1 ACCEPTED / 2 REJECTED。</summary>
    [Builtin(ABI = "rt_crypto_tls_early_data_status")]
    private int _EarlyDataStatus() { return 0; }

    // ── 异步握手（内存 BIO · Reactor 真异步字节面；RFC 009 异步为主 · 不阻塞调用线程）──

    /// <summary>TLS 1.3 全握手（真异步：字节面 I/O 经
    /// <see cref="TcpClient.SendBytesAsync"/>/<see cref="TcpClient.ReceiveBytesAsync"/>
    /// （Reactor 提交 read/write）await，不阻塞调用线程）。握手工作直接在本 async
    /// 方法内执行（Task.Run 委托包装下异常无法经 C trampoline 展开——语言缺口，
    /// 见 S5 验收注记），故直接 await 内部真异步握手体。失败抛异常。</summary>
    public async Task AuthenticateAsClientAsync() {
        await this.DoHandshakeAsync();
    }

    /// <summary>TLS 1.3 全握手（同步；P1 同步路径消费面——同步传输下与 async 等价，
    /// 供 WebSocketClient wss 桥接等同步面复用；失败抛异常）。</summary>
    public void Authenticate() {
        this.DoHandshake();
    }

    private void DoHandshake() {
        if (_authenticated) {
            return;
        }
        int mode = this.ResolveVerifyMode();
        byte[] trustDer = ZeroBytes(0);
        if (_trustAnchor != null && _trustAnchor.RawData != null) {
            trustDer = _trustAnchor.RawData;
        }
        byte[] alpnBlob = this.BuildAlpnBlob(_appProtocols);
        long h = _ClientNew(_targetHost, trustDer, alpnBlob);
        if (h == 0) {
            throw new InvalidOperationException("TlsClientSession: failed to create TLS session.");
        }
        _handle = h;

        // S5 握手前配置：校验策略 / 吊销 / 双向认证 / 会话恢复 / 0-RTT。
        // 语言缺口：byte[] 类字段不支持 `.Length`（TypeId 解析为 byte_arr）→ 先拷本地。
        if (mode != 0) {
            bool hasAnchors = _trustAnchor != null
                || (_trustAnchors != null && _trustAnchors.Count > 0);
            if (mode == 2 && !hasAnchors && _useSystemRoots) {
                // 默认 FullChain：无显式锚 → 载入 OS 系统根证书（真实公网主机证书校验）。
                if (this._LoadSystemRoots() != 0) {
                    this.Abort("load_system_roots failed.");
                }
            } else {
                byte[] caBlob = ZeroBytes(0);
                if (mode == 2) {
                    caBlob = this.BuildTrustChainBlob();
                } else if (trustDer.Length > 0) {
                    caBlob = trustDer;
                }
                if (this._SetVerify(mode, caBlob) != 0) {
                    this.Abort("set_verify failed.");
                }
            }
        }
        byte[] crlData = _crlData;
        if (crlData != null && crlData.Length > 0) {
            if (this._SetCrl(crlData) != 0) {
                this.Abort("set_crl failed.");
            }
        }
        if (_clientCert != null && _clientKey != null) {
            if (this._SetClientCert(_clientCert.RawData, _clientKey) != 0) {
                this.Abort("set_client_cert failed.");
            }
        }
        byte[] sessionBytes = _sessionBytes;
        if (sessionBytes != null && sessionBytes.Length > 0) {
            // 恢复失败不中断：mbedTLS 语义仅复位会话 → 退全握手。
            this._SessionLoad(sessionBytes);
        }
        if (_earlyDataEnabled) {
            this._EnableEarlyData(1);
        }

        TcpClient cl = _stream.BaseClient;
        int state = 0;
        byte[] recv = ZeroBytes(0);

        // S5 0-RTT：早数据载荷就绪 → 先走早数据写循环（成功写出或退正常握手）。
        byte[] earlyData = _earlyDataPayload;
        if (_earlyDataEnabled && earlyData != null && earlyData.Length > 0) {
            int edState = -2;
            while (true) {
                byte[] sendOut = this._WriteEarlyData(recv, earlyData, out edState);
                if (sendOut == null) {
                    if (edState == -2) {
                        throw new IOException("TLS 1.3 early data write failed.");
                    }
                    sendOut = ZeroBytes(0);
                }
                if (sendOut.Length > 0) {
                    int sent = cl.SendBytes(sendOut, 0, sendOut.Length);
                    if (sent != sendOut.Length) {
                        throw new IOException("TLS 1.3 early data output send failed.");
                    }
                }
                if (edState == 1 || edState == -1) {
                    // 早数据已写出（或退正常握手）。上一轮 recv 已被该次
                    // _WriteEarlyData 推入输入 FIFO——复位为空，防正常握手
                    // 循环重复喂入同一块。
                    recv = ZeroBytes(0);
                    break;
                }
                if (edState == -2) {
                    throw new IOException("TLS 1.3 early data write failed.");
                }
                // edState == 0（WANT_READ）：读下一块再喂。
                byte[] buf = ZeroBytes(4096);
                int n = cl.ReceiveBytes(buf, 0, 4096);
                if (n == 0) {
                    throw new IOException("TLS 1.3 early data input EOF.");
                }
                if (n < 0) {
                    // 非阻塞传输无数据就绪：让出后重试早数据循环。
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
        }

        while (state != 1) {
            byte[] sendOut = this._Handshake(recv, out state);
            if (sendOut == null) {
                throw new IOException("TLS 1.3 handshake failed.");
            }
            if (sendOut.Length > 0) {
                int sent = cl.SendBytes(sendOut, 0, sendOut.Length);
                if (sent != sendOut.Length) {
                    throw new IOException("TLS 1.3 handshake output send failed.");
                }
            }
            if (state == 1) {
                break;
            }
            byte[] buf = ZeroBytes(4096);
            int n = cl.ReceiveBytes(buf, 0, 4096);
            if (n == 0) {
                throw new IOException("TLS 1.3 handshake input EOF.");
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
        _authenticated = true;
        _negotiated = this._Alpn();
        _earlyDataStatus = this._EarlyDataStatus();
    }

    /* 真异步握手（RFC 009 异步为主 · WebSocket wss 全量真异步）：与同步
     * DoHandshake 逻辑等价，但字节面 I/O 经 `TcpClient.SendBytesAsync /
     * ReceiveBytesAsync`（Reactor 真异步）await，不阻塞调用线程。
     * 同步 DoHandshake 保留供 WebTransport / AI 同步面复用（不破坏既有消费面）。 */
    private async Task DoHandshakeAsync() {
        if (_authenticated) {
            return;
        }
        int mode = this.ResolveVerifyMode();
        byte[] trustDer = ZeroBytes(0);
        if (_trustAnchor != null && _trustAnchor.RawData != null) {
            trustDer = _trustAnchor.RawData;
        }
        byte[] alpnBlob = this.BuildAlpnBlob(_appProtocols);
        long h = _ClientNew(_targetHost, trustDer, alpnBlob);
        if (h == 0) {
            throw new InvalidOperationException("TlsClientSession: failed to create TLS session.");
        }
        _handle = h;

        if (mode != 0) {
            bool hasAnchors = _trustAnchor != null
                || (_trustAnchors != null && _trustAnchors.Count > 0);
            if (mode == 2 && !hasAnchors && _useSystemRoots) {
                if (this._LoadSystemRoots() != 0) {
                    this.Abort("load_system_roots failed.");
                }
            } else {
                byte[] caBlob = ZeroBytes(0);
                if (mode == 2) {
                    caBlob = this.BuildTrustChainBlob();
                } else if (trustDer.Length > 0) {
                    caBlob = trustDer;
                }
                if (this._SetVerify(mode, caBlob) != 0) {
                    this.Abort("set_verify failed.");
                }
            }
        }
        byte[] crlData = _crlData;
        if (crlData != null && crlData.Length > 0) {
            if (this._SetCrl(crlData) != 0) {
                this.Abort("set_crl failed.");
            }
        }
        if (_clientCert != null && _clientKey != null) {
            if (this._SetClientCert(_clientCert.RawData, _clientKey) != 0) {
                this.Abort("set_client_cert failed.");
            }
        }
        byte[] sessionBytes = _sessionBytes;
        if (sessionBytes != null && sessionBytes.Length > 0) {
            this._SessionLoad(sessionBytes);
        }
        if (_earlyDataEnabled) {
            this._EnableEarlyData(1);
        }

        TcpClient cl = _stream.BaseClient;
        int state = 0;
        byte[] recv = ZeroBytes(0);

        /* S5 0-RTT：早数据载荷就绪 → 先走早数据写循环（成功写出或退正常握手）。 */
        byte[] earlyData = _earlyDataPayload;
        if (_earlyDataEnabled && earlyData != null && earlyData.Length > 0) {
            int edState = -2;
            while (true) {
                byte[] sendOut = this._WriteEarlyData(recv, earlyData, out edState);
                if (sendOut == null) {
                    if (edState == -2) {
                        throw new IOException("TLS 1.3 early data write failed.");
                    }
                    sendOut = ZeroBytes(0);
                }
                if (sendOut.Length > 0) {
                    int sent = await cl.SendBytesAsync(sendOut, 0, sendOut.Length);
                    if (sent != sendOut.Length) {
                        throw new IOException("TLS 1.3 early data output send failed.");
                    }
                }
                if (edState == 1 || edState == -1) {
                    recv = ZeroBytes(0);
                    break;
                }
                if (edState == -2) {
                    throw new IOException("TLS 1.3 early data write failed.");
                }
                byte[] buf = ZeroBytes(4096);
                int n = await cl.ReceiveBytesAsync(buf, 0, 4096);
                if (n <= 0) {
                    throw new IOException("TLS 1.3 early data input EOF.");
                }
                byte[] next = ZeroBytes(n);
                for (int i = 0; i < n; i++) {
                    next[i] = buf[i];
                }
                recv = next;
            }
        }

        while (state != 1) {
            byte[] sendOut = this._Handshake(recv, out state);
            if (sendOut == null) {
                throw new IOException("TLS 1.3 handshake failed.");
            }
            if (sendOut.Length > 0) {
                int sent = await cl.SendBytesAsync(sendOut, 0, sendOut.Length);
                if (sent != sendOut.Length) {
                    throw new IOException("TLS 1.3 handshake output send failed.");
                }
            }
            if (state == 1) {
                break;
            }
            byte[] buf = ZeroBytes(4096);
            int n = await cl.ReceiveBytesAsync(buf, 0, 4096);
            if (n <= 0) {
                throw new IOException("TLS 1.3 handshake input EOF.");
            }
            byte[] next = ZeroBytes(n);
            for (int i = 0; i < n; i++) {
                next[i] = buf[i];
            }
            recv = next;
        }
        _authenticated = true;
        _negotiated = this._Alpn();
        _earlyDataStatus = this._EarlyDataStatus();
    }

    // ── 加密字节流读写（语义对齐 NetworkStream.Read / Write）──

    /// <summary>解密明文读（byte[] 面）。返回实际字节数；EOF（close_notify）返回 0。</summary>
    public int Read(byte[] buffer, int offset, int count) {
        if (!_authenticated) {
            throw new InvalidOperationException("TlsClientSession is not authenticated.");
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
            int n = this._Read(empty, buffer, offset, count);
            if (n == -2) {
                byte[] buf = ZeroBytes(4096);
                int r = cl.ReceiveBytes(buf, 0, 4096);
                if (r == 0) {
                    return 0;
                }
                if (r < 0) {
                    // 非阻塞传输（异步握手后 socket 保持非阻塞）无数据就绪：
                    // rt_net_recv 以 -1 区分 WANT_WRITE/WOULDBLOCK，短暂让出后重试。
                    Thread.Sleep(1);
                    continue;
                }
                byte[] enc = ZeroBytes(r);
                for (int i = 0; i < r; i++) {
                    enc[i] = buf[i];
                }
                n = this._Read(enc, buffer, offset, count);
                if (n == -2) {
                    // 喂入密文后仍 WANT_READ：密文可能只是 post-handshake 消息
                    // （NewSessionTicket 等，mbedTLS 消费后以 WANT_READ 交还控制），
                    // 应用数据仍排队在输入 FIFO。空读连续排空，再回落 transport 读。
                    for (int drain = 0; drain < 4 && n == -2; drain++) {
                        n = this._Read(empty, buffer, offset, count);
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
            throw new InvalidOperationException("TlsClientSession is not authenticated.");
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
        byte[] enc = this._Write(plain);
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

    // ── 真异步加密字节流读写（RFC 009 异步为主 · Reactor 字节面 await；
    //    语义对齐同步 Read/Write · WebSocketClient wss 全量真异步消费面）──

    /// <summary>解密明文读（byte[] 面；真异步）。返回实际字节数；EOF（close_notify）返回 0。
    /// 经 <see cref="TcpClient.ReceiveBytesAsync"/> await，不阻塞调用线程。</summary>
    public async Task<int> ReadAsync(byte[] buffer, int offset, int count) {
        if (!_authenticated) {
            throw new InvalidOperationException("TlsClientSession is not authenticated.");
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
            int n = this._Read(empty, buffer, offset, count);
            if (n == 0) {
                return 0;
            }
            if (n == -2) {
                byte[] buf = ZeroBytes(4096);
                int r = await cl.ReceiveBytesAsync(buf, 0, 4096);
                if (r == 0) {
                    return 0;
                }
                if (r < 0) {
                    // 非阻塞传输无数据就绪：短暂让出后重试 WANT_READ。
                    await Task.Delay(1);
                    continue;
                }
                byte[] enc = ZeroBytes(r);
                for (int i = 0; i < r; i++) {
                    enc[i] = buf[i];
                }
                n = this._Read(enc, buffer, offset, count);
                if (n == -2) {
                    // 同同步 Read：密文可能仅含 post-handshake 消息（NewSessionTicket 等），
                    // mbedTLS 消费后以 WANT_READ 交还；空读排空排队数据后再回落 transport。
                    for (int drain = 0; drain < 4 && n == -2; drain++) {
                        n = this._Read(empty, buffer, offset, count);
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

    /// <summary>明文写 → 加密发送（全量；真异步；失败抛 IOException）。
    /// 经 <see cref="TcpClient.SendBytesAsync"/> await，不阻塞调用线程。</summary>
    public async Task WriteAsync(byte[] buffer, int offset, int count) {
        if (!_authenticated) {
            throw new InvalidOperationException("TlsClientSession is not authenticated.");
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
        byte[] enc = this._Write(plain);
        if (enc == null) {
            throw new IOException("TLS write failed.");
        }
        if (enc.Length == 0) {
            throw new IOException("TLS write requires inbound data (WANT_READ); retry after Read.");
        }
        TcpClient cl = _stream.BaseClient;
        int sent = await cl.SendBytesAsync(enc, 0, enc.Length);
        if (sent != enc.Length) {
            throw new IOException("TLS write send failed.");
        }
    }

    /// <summary>释放 TLS 会话句柄与底层传输。</summary>
    public void Dispose() {
        if (_handle != 0) {
            this._Free();
            _handle = 0;
        }
        if (_stream != null) {
            _stream.Close();
        }
    }

    // ── 私有工具 ──

    /// <summary>校验策略解析：显式 VerifyMode 优先；未显式时按锚存在与否兼容演进；
    /// 两者皆无 → 默认 FullChain（真实公网主机 · 经系统根或显式锚校验）。</summary>
    private int ResolveVerifyMode() {
        if (_verifyModeSet == 1) {
            if (_verifyMode == TlsCertificateVerification.None) {
                return 0;
            }
            if (_verifyMode == TlsCertificateVerification.Anchor) {
                return 1;
            }
            return 2;
        }
        if (_trustAnchor != null) {
            return 1;
        }
        if (_trustAnchors != null && _trustAnchors.Count > 0) {
            return 2;
        }
        return 2;
    }

    /// <summary>FullChain 模式信任链 blob：根+中间 PEM 拼接；无列表时回退单锚 DER。</summary>
    private byte[] BuildTrustChainBlob() {
        List<byte> blob = new List<byte>();
        List<X509Certificate2> anchors = _trustAnchors;
        if (anchors != null && anchors.Count > 0) {
            for (int i = 0; i < anchors.Count; i++) {
                X509Certificate2 c = anchors[i];
                byte[] der = (c != null) ? c.RawData : null;
                if (der == null || der.Length == 0) {
                    continue;
                }
                for (int j = 0; j < der.Length; j++) {
                    blob.Add(der[j]);
                }
            }
            if (blob.Count > 0) {
                return blob.ToArray();
            }
        }
        if (_trustAnchor != null && _trustAnchor.RawData != null) {
            return _trustAnchor.RawData;
        }
        return ZeroBytes(0);
    }

    /// <summary>配置失败清理并抛异常。</summary>
    private void Abort(string message) {
        if (_handle != 0) {
            this._Free();
            _handle = 0;
        }
        throw new InvalidOperationException("TlsClientSession: " + message);
    }

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
