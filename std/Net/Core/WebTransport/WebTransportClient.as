// RFC 039 W1/W2: Arc.Net.WebTransport — WebTransport 客户端。
//
// 协议锚定（039 §0/§1.1）：
//   - W1 = WebTransport over HTTP/2：HTTP/2 extended CONNECT（RFC 8441，
//     :protocol = webtransport）+ RFC 9297 capsule 协议承载 datagram 与流映射
//     （draft-ietf-webtrans-http2-15）。会话建立在 CONNECT 流上，Session ID =
//     CONNECT 流的 HTTP/2 流 ID；会话内流/数据报全部经 capsule 复用。
//   - W2 = WebTransport over HTTP/3（draft-ietf-webtrans-http3-16）：本机字节面
//     编解码见 WebTransportCodec；QUIC 传输绑定受编译器 .ani 契约面限制，由 e2e
//     harness 直调 rt_quic_* 承载（诚实边界，见 039 W1/W2 验收注记）。
//
// 分流（039 §1.1 回退映射）：
//   - https:// → 先尝试 W2（HTTP/3）：当前 Arc 侧无 QUIC 传输绑定 → 记录后回落 W1；
//   - wss:// / https://（回落） → W1 over TLS（S0 TlsClientSession · ALPN h2）；
//   - ws:// → W1 明文 h2c（本地闭环验收测试形态，对齐 S2 http2_h2c_e2e；
//     在 039 W1/W2 验收注记记录）；
//   - http:// 及其它 → 明确拒绝（039 §1.3）。
//
// 语言能力缺口（不改语言）：`byte[]` 字段直读不支持 .Length/索引 → 先拷贝局部；
// `long` 字面量不支持 `L` 后缀 → 常数经 `(long)` 显式 cast；位运算缺失 → 标志位
// 以 `flags / bit % 2` 仿真（与 Hpack/WebSocket 同例）。
namespace Arc.Net.WebTransport;

using Arc;
using Arc.Collections;
using Arc.Net;
using Arc.Net.Security;
using Arc.Security.Cryptography;
using Arc.Text;

public class WebTransportClient : IDisposable {
    private WebTransportState _state;
    private int _maxDatagramSize;
    private TcpClient _tcp;
    private NetworkStream _stream;
    private TlsClientSession _tls;
    private List<string> _alpnH2;
    private X509Certificate2 _trustAnchor;

    // W1 会话状态
    private List<byte> _sessionIn;
    private bool _sessionEnded;
    private int _closeCode;
    private string _closeMessage;
    private bool _closeReplySeen;

    // 流 / 数据报状态
    private List<WebTransportStream> _streams;
    private int _acceptCursor;
    private int _nextClientBidi;
    private int _nextClientUni;
    private List<byte> _datagramQueue;
    private bool _datagramPending;

    // 流控（最小实现：记录对端授权，本地闭环不阻塞发送）
    private long _maxDataGranted;
    private long _maxStreamDataGranted;

    private bool _disposed;

    /// <summary>创建未连接的 WebTransport 客户端（默认 ALPN ["h3","h2"]）。</summary>
    public WebTransportClient() {
        _state = WebTransportState.None;
        _maxDatagramSize = 16379; // W1：HTTP/2 帧上限 16384 − DATAGRAM capsule 头（type+len varint）
        _alpnH2 = new List<string>();
        _alpnH2.Add("h2");
        _sessionIn = new List<byte>();
        _closeMessage = "";
        _streams = new List<WebTransportStream>();
        _nextClientUni = 2;
        _datagramQueue = new List<byte>();
    }

    /// <summary>当前连接状态（None/Connecting/Connected/Closing/Closed/Failed）。</summary>
    public WebTransportState State {
        get { return _state; }
    }

    /// <summary>数据报大小上限（超限 SendDatagramAsync 失败，不静默截断）。</summary>
    public int MaxDatagramSize {
        get { return _maxDatagramSize; }
    }

    // ── 会话建立 ──

    /// <summary>连接 https:// 或 wss:// URL（内部分流 HTTP/3 或 HTTP/2 映射）；
    /// ws:// 为明文 h2c 本地闭环测试形态。成功返回 true。</summary>
    public Task<bool> ConnectAsync(string url) {
        return Task.FromResult(this.DoConnect(url));
    }

    private bool DoConnect(string url) {
        if (_disposed) { return false; }
        _state = WebTransportState.Connecting;
        bool ok = false;
        if (url == null) { ok = false; }
        else if (url.StartsWith("https://")) {
            // W2（HTTP/3）为会话主映射：先尝试 H3。当前 Arc 侧 QUIC 传输绑定
            // 未立宪（编译器 .ani 契约面限制），尝试即记录并回落 W1（诚实边界
            // 见 039 W1/W2 验收注记）。
            if (this.TryConnectW2(url)) { ok = true; }
            else { ok = this.DoConnectW1(url, true); }
        } else if (url.StartsWith("wss://")) {
            ok = this.DoConnectW1(url, true);
        } else if (url.StartsWith("ws://")) {
            ok = this.DoConnectW1(url, false);
        } else {
            ok = false; // http:// 等明确拒绝（039 §1.3）
        }
        if (!ok) {
            if (_state == WebTransportState.Connecting) { _state = WebTransportState.Failed; }
            return false;
        }
        _state = WebTransportState.Connected;
        return true;
    }

    /// <summary>W2 尝试：QUIC 传输绑定缺口（诚实边界）——Arc 侧 WebTransportClient
    /// 不启动 ngtcp2 会话（rt_quic_* 由 harness 直调），返回 false → 回落 W1。</summary>
    private bool TryConnectW2(string url) {
        return false;
    }

    /// <summary>W1：HTTP/2 会话建立（extended CONNECT :protocol=webtransport）。</summary>
    private bool DoConnectW1(string url, bool secure) {
        string host = "";
        int port = 80;
        string path = "/";
        if (!this.ParseUrl(url, secure, ref host, ref port, ref path)) { return false; }

        TcpClient cl = new TcpClient();
        cl.SetReceiveTimeout(5000);
        cl.SetSendTimeout(5000);
        cl.SetNoDelay(true);
        if (!cl.Connect(host, port)) { cl.Close(); return false; }
        NetworkStream ns = new NetworkStream(cl, 5000);
        _tcp = cl;
        _stream = ns;

        if (secure) {
            // wss/https（W1 over TLS · S0）：字节层桥接为 TlsClientSession（ALPN h2）。
            TlsClientSession tls = new TlsClientSession(ns);
            tls.TargetHost = host;
            tls.ApplicationProtocols = _alpnH2;
            tls.TrustAnchor = _trustAnchor;
            try {
                tls.Authenticate();
            } catch (Exception ex) {
                tls.Dispose();
                cl.Close();
                return false;
            }
            if (!tls.IsAuthenticated) {
                tls.Dispose();
                cl.Close();
                return false;
            }
            _tls = tls;
        }

        // HTTP/2 前置：PRI preface + 客户端 SETTINGS（SETTINGS_ENABLE_CONNECT_PROTOCOL=1）。
        if (!this.SendH2Preface()) { this.Close(); return false; }
        // 服务器 SETTINGS：须协商 SETTINGS_ENABLE_CONNECT_PROTOCOL=1 且 SETTINGS_WT_ENABLED=1。
        if (!this.ReadServerSettingsW1()) { this.Close(); return false; }
        // extended CONNECT（RFC 8441 · :protocol=webtransport）→ CONNECT 流（Stream 1）。
        string authority = host + ":" + port.ToString();
        byte[] hb = WebTransportCodec.BuildH2ConnectHeaderBlock(authority, path);
        Http2Frame hf = Http2Frame.MakeHeaders(1, false, hb);
        if (!this.WriteFrameW1(hf)) { this.Close(); return false; }
        int status = this.WaitForSessionW1();
        if (status < 200 || status >= 300) { this.Close(); return false; }

        _sessionEnded = false;
        _closeReplySeen = false;
        // 会话建立后授予流控（本地闭环：充分授权）。
        byte[] md = WebTransportCapsule.MakeMaxData((long)1073741824);
        this.SendConnectDataW1(md);
        return true;
    }

    // ── 双向 / 单向流 ──

    /// <summary>打开客户端→服务器双向流（W1 流号 0,4,8,…）。未连接返回 null。</summary>
    public Task<WebTransportStream> OpenBidirectionalStreamAsync() {
        return Task.FromResult(this.OpenStream(true));
    }

    /// <summary>打开客户端→服务器单向流（W1 流号 2,6,10,…）。未连接返回 null。</summary>
    public Task<WebTransportStream> OpenUnidirectionalStreamAsync() {
        return Task.FromResult(this.OpenStream(false));
    }

    /// <summary>接受服务器发起的双向流（等对端 WT_STREAM capsule）。</summary>
    public Task<WebTransportStream> AcceptBidirectionalStreamAsync() {
        return Task.FromResult(this.AcceptStream());
    }

    /// <summary>接受服务器发起的单向流（等对端 WT_STREAM capsule）。</summary>
    public Task<WebTransportStream> AcceptUnidirectionalStreamAsync() {
        return Task.FromResult(this.AcceptStream());
    }

    private WebTransportStream OpenStream(bool bidi) {
        if (_state != WebTransportState.Connected) { return null; }
        int sid = bidi ? _nextClientBidi : _nextClientUni;
        if (bidi) { _nextClientBidi = _nextClientBidi + 4; }
        else { _nextClientUni = _nextClientUni + 4; }
        this.GrantStreamFlowControl((long)sid);
        WebTransportStream s = new WebTransportStream(this, sid);
        _streams.Add(s);
        return s;
    }

    private WebTransportStream AcceptStream() {
        while (true) {
            int i = _acceptCursor;
            while (i < _streams.Count) {
                WebTransportStream s = _streams[i];
                if (s.IsPeerInitiated && !s.Accepted) {
                    s.Accepted = true;
                    _acceptCursor = i + 1;
                    return s;
                }
                i = i + 1;
            }
            _acceptCursor = _streams.Count;
            if (_sessionEnded || _state != WebTransportState.Connected) { return null; }
            this.PumpW1(64);
        }
    }

    // ── 数据报 ──

    /// <summary>发送数据报（W1：DATAGRAM capsule）；返回发送字节数，超限/未连接返回 0。</summary>
    public Task<int> SendDatagramAsync(byte[] data) {
        return Task.FromResult(this.DoSendDatagram(data));
    }

    private int DoSendDatagram(byte[] data) {
        if (_state != WebTransportState.Connected || data == null) { return 0; }
        if (data.Length > _maxDatagramSize) { return 0; }
        byte[] cap = WebTransportCapsule.MakeDatagram(data);
        if (this.SendConnectDataW1(cap)) { return data.Length; }
        return 0;
    }

    /// <summary>接收单条数据报；超时/会话关闭返回 null。</summary>
    public Task<byte[]> ReceiveDatagramAsync() {
        return Task.FromResult(this.DoReceiveDatagram());
    }

    private byte[] DoReceiveDatagram() {
        if (_state != WebTransportState.Connected) { return null; }
        int poll = 0;
        while (!_datagramPending && poll < 100) {
            this.PumpW1(64);
            if (_sessionEnded) { return null; }
            poll = poll + 1;
        }
        if (!_datagramPending) { return null; }
        byte[] dg = _datagramQueue.ToArray();
        _datagramQueue.Clear();
        _datagramPending = false;
        return dg;
    }

    // ── 会话关闭 ──

    /// <summary>会话关闭握手：发送 WT_CLOSE_SESSION capsule + CONNECT 流 END_STREAM，
    /// 等对端 WT_CLOSE_SESSION 响应后关闭；成功返回 true。</summary>
    public Task<bool> CloseAsync(int closeCode, string reason) {
        return Task.FromResult(this.DoCloseSession(closeCode, reason));
    }

    private bool DoCloseSession(int closeCode, string reason) {
        if (_state != WebTransportState.Connected) { return false; }
        _state = WebTransportState.Closing;
        _closeReplySeen = false;
        byte[] cap = WebTransportCapsule.MakeCloseSession(closeCode, reason);
        Http2Frame fin = Http2Frame.MakeData(1, true, cap);
        if (!this.WriteFrameW1(fin)) { this.Close(); _state = WebTransportState.Failed; return false; }
        int poll = 0;
        while (poll < 400000 && !_closeReplySeen) {
            this.PumpW1(32);
            if (_sessionEnded) { break; }
            poll = poll + 1;
        }
        this.Close();
        _state = WebTransportState.Closed;
        return _closeReplySeen;
    }

    /// <summary>硬关闭底层连接（不发送关闭 capsule）。</summary>
    public void Close() {
        if (_tls != null) {
            _tls.Dispose();
            _tls = null;
        }
        // TcpClient 持有 socket handle 的所有权（rt_socket_close 即 free）。
        // NetworkStream.Close 已委托关闭底层 TcpClient（_stream._client 与 _tcp 为同一
        // 对象）。此处只释放一次：关 _stream（委托）后即置空 _tcp，避免对同一 handle
        // 二次 rt_socket_close → 双重释放 / UAF / 0xC0000374 堆损坏（ASan 捕获）。
        if (_stream != null) {
            _stream.Close();
            _stream = null;
            _tcp = null;
        } else if (_tcp != null) {
            _tcp.Close();
            _tcp = null;
        }
        if (_state == WebTransportState.Connected || _state == WebTransportState.Closing) {
            _state = WebTransportState.Closed;
        }
    }

    public void Dispose() {
        _disposed = true;
        this.Close();
    }

    // ── Internal（WebTransportStream 回调） ──

    /// <summary>流写：立即组 WT_STREAM capsule 发送。</summary>
    internal bool StreamWrite(int sid, byte[] buffer, int offset, int count) {
        if (_state != WebTransportState.Connected) { return false; }
        List<byte> data = new List<byte>();
        int i = 0;
        while (i < count) {
            data.Add(buffer[offset + i]);
            i = i + 1;
        }
        byte[] cap = WebTransportCapsule.MakeStream(false, (long)sid, data.ToArray());
        return this.SendConnectDataW1(cap);
    }

    /// <summary>流关闭：发送 WT_STREAM_FIN 并等对端同流 FIN 响应。</summary>
    internal bool StreamCloseRequest(int sid) {
        if (_state != WebTransportState.Connected) { return false; }
        byte[] cap = WebTransportCapsule.MakeStream(true, (long)sid, Http2ByteUtils.ZeroBytes(0));
        if (!this.SendConnectDataW1(cap)) { return false; }
        WebTransportStream st = this.FindStream(sid);
        int poll = 0;
        while (poll < 200) {
            if (st != null && st.IsReadComplete) { return true; }
            this.PumpW1(32);
            if (_sessionEnded) { return st != null && st.IsReadComplete; }
            poll = poll + 1;
        }
        return st != null && st.IsReadComplete;
    }

    // ── W1 帧/传输层 ──

    private bool SendH2Preface() {
        string preface = "PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";
        byte[] pb = Encoding.GetBytes(preface);
        if (!this.WriteBytesW1(pb)) { return false; }
        Http2Frame sf = Http2Frame.MakeSettings([0x08], [1]);
        return this.WriteFrameW1(sf);
    }

    /// <summary>服务器 SETTINGS 协商：ENABLE_CONNECT_PROTOCOL=1 且 WT_ENABLED(0x2b60)=1，
    /// 并回 SETTINGS ACK。未协商返回 false。</summary>
    private bool ReadServerSettingsW1() {
        int guard = 0;
        while (guard < 64) {
            Http2Frame f = this.ReadFrameW1();
            if (f == null) { return false; }
            if (f.Type == Http2FrameTypes.Settings) {
                bool ack = f.Flags / Http2FrameTypes.FlagAck % 2 == 1;
                if (!ack) {
                    byte[] pl = f.Payload;
                    int off = 0;
                    bool ecp = false;
                    bool wte = false;
                    while (off + 6 <= pl.Length) {
                        int id = (pl[off] * 256) + pl[off + 1];
                        long val = ((long)pl[off + 2] * 16777216) + ((long)pl[off + 3] * 65536) + ((long)pl[off + 4] * 256) + (long)pl[off + 5];
                        if (id == 0x08 && val == 1) { ecp = true; }
                        if (id == 0x2b60 && val == 1) { wte = true; }
                        off = off + 6;
                    }
                    if (!ecp || !wte) { return false; }
                    this.WriteFrameW1(Http2Frame.MakeSettingsAck());
                    return true;
                }
            } else if (f.Type == Http2FrameTypes.Ping) {
                if (f.Flags / Http2FrameTypes.FlagAck % 2 == 0) {
                    this.SendPingAckW1(f.Payload);
                }
            } else if (f.Type == Http2FrameTypes.GoAway) {
                return false;
            }
            guard = guard + 1;
        }
        return false;
    }

    /// <summary>等 CONNECT 流响应 HEADERS；返回 :status（失败返回 0）。</summary>
    private int WaitForSessionW1() {
        int guard = 0;
        while (guard < 64) {
            Http2Frame f = this.ReadFrameW1();
            if (f == null) { return 0; }
            if (f.Type == Http2FrameTypes.Headers && f.StreamId == 1) {
                byte[] payload = f.Payload;
                Hpack hp = new Hpack();
                Http2HeaderList hs = new Http2HeaderList();
                bool dec = hp.DecodeHeaders(payload, hs);
                if (!dec) { return 0; }
                string status = hs.Get(":status");
                if (status == "") { return 0; }
                if (f.Flags / Http2FrameTypes.FlagEndStream % 2 == 1) { _sessionEnded = true; }
                return Convert.ToInt32(status);
            }
            if (f.Type == Http2FrameTypes.Ping) {
                if (f.Flags / Http2FrameTypes.FlagAck % 2 == 0) { this.SendPingAckW1(f.Payload); }
            }
            if (f.Type == Http2FrameTypes.GoAway) { return 0; }
            guard = guard + 1;
        }
        return 0;
    }

    /// <summary>连接读循环：读帧分派（DATA→capsule、PING→应答、GOAWAY→会话结束）。
    ///
    /// 非阻塞轮询：每轮先探测可读字节，无数据立即返回（不阻塞、不误判 EOF）。
    /// 有数据才读帧——避免阻塞读在无待发帧时超时被误判为会话关闭（本地闭环
    /// 关键：读空对端批量后保持 Connected，供后续数据报/流操作继续）。</summary>
    private void PumpW1(int budget) {
        int i = 0;
        while (i < budget) {
            if (!this.DataAvailableW1()) { return; }
            Http2Frame f = this.ReadFrameW1();
            if (f == null) {
                _sessionEnded = true;
                if (_state == WebTransportState.Connected || _state == WebTransportState.Closing) {
                    _state = WebTransportState.Closed;
                }
                return;
            }
            this.HandleIncomingFrameW1(f);
            i = i + 1;
        }
    }

    /// <summary>单帧分派（Data / Headers / Ping / GoAway on CONNECT 流）。由
    /// <see cref="PumpW1"/>（非阻塞）与关闭握手的阻塞读路径共用，避免重复分派逻辑。</summary>
    private void HandleIncomingFrameW1(Http2Frame f) {
        if (f.Type == Http2FrameTypes.Data && f.StreamId == 1) {
            byte[] pl = f.Payload;
            int k = 0;
            while (k < pl.Length) {
                _sessionIn.Add(pl[k]);
                k = k + 1;
            }
            if (f.Flags / Http2FrameTypes.FlagEndStream % 2 == 1) { _sessionEnded = true; }
            this.ParseSessionCapsules();
        } else if (f.Type == Http2FrameTypes.Headers && f.StreamId == 1) {
            if (f.Flags / Http2FrameTypes.FlagEndStream % 2 == 1) { _sessionEnded = true; }
        } else if (f.Type == Http2FrameTypes.Ping) {
            if (f.Flags / Http2FrameTypes.FlagAck % 2 == 0) { this.SendPingAckW1(f.Payload); }
        } else if (f.Type == Http2FrameTypes.GoAway) {
            _sessionEnded = true;
            if (_state == WebTransportState.Closing) { _closeReplySeen = true; }
        }
    }

    /// <summary>从 CONNECT 流累积字节解析 capsule 序列并分派（不完整即等更多数据）。</summary>
    private void ParseSessionCapsules() {
        while (true) {
            if (_sessionIn.Count < 2) { return; }
            byte[] tmp = _sessionIn.ToArray();
            int l1;
            long type = WebTransportVarInt.Decode(tmp, 0, out l1);
            if (type < 0) { return; }
            if (_sessionIn.Count < l1 + 1) { return; }
            int l2;
            long clen = WebTransportVarInt.Decode(tmp, l1, out l2);
            if (clen < 0) { return; }
            int total = l1 + l2 + (int)clen;
            if (_sessionIn.Count < total) { return; }
            byte[] payload = this.SubBytes(tmp, l1 + l2, (int)clen);
            this.HandleCapsule(type, payload);
            List<byte> remaining = new List<byte>();
            int r = total;
            while (r < _sessionIn.Count) {
                remaining.Add(_sessionIn[r]);
                r = r + 1;
            }
            _sessionIn = remaining;
        }
    }

    /// <summary>单 capsule 分派。</summary>
    private void HandleCapsule(long type, byte[] payload) {
        if (type == WebTransportCapsuleTypes.Datagram) {
            _datagramQueue.Clear();
            int i = 0;
            while (i < payload.Length) {
                _datagramQueue.Add(payload[i]);
                i = i + 1;
            }
            _datagramPending = true;
        } else if (type == WebTransportCapsuleTypes.Stream || type == WebTransportCapsuleTypes.StreamFin) {
            int sidLen;
            long sid = WebTransportVarInt.Decode(payload, 0, out sidLen);
            if (sid < 0) { return; }
            WebTransportStream st = this.FindStream((int)sid);
            if (st == null) {
                st = new WebTransportStream(this, (int)sid);
                st.IsPeerInitiated = true;
                _streams.Add(st);
            }
            int k = sidLen;
            while (k < payload.Length) {
                st.DeliverChunk(payload[k]);
                k = k + 1;
            }
            if (type == WebTransportCapsuleTypes.StreamFin) { st.MarkFin(); }
        } else if (type == WebTransportCapsuleTypes.CloseSession) {
            int code;
            string msg = "";
            if (WebTransportCapsule.ParseCloseSessionPayload(payload, out code, out msg)) {
                _closeCode = code;
                _closeMessage = msg;
                _closeReplySeen = true;
            }
        } else if (type == WebTransportCapsuleTypes.MaxData) {
            int l;
            long v = WebTransportVarInt.Decode(payload, 0, out l);
            if (v >= 0) { _maxDataGranted = v; }
        } else if (type == WebTransportCapsuleTypes.MaxStreamData) {
            int l;
            long v = WebTransportVarInt.Decode(payload, 0, out l);
            if (v >= 0) { _maxStreamDataGranted = v; }
        }
        // WT_RESET_STREAM / WT_STOP_SENDING / 流控阻塞类 capsule：本地闭环不触发，忽略。
    }

    // ── Private: 传输原语 ──

    private bool WriteFrameW1(Http2Frame f) {
        byte[] raw = f.Encode();
        if (raw == null) { return false; }
        return this.WriteBytesW1(raw);
    }

    private bool SendConnectDataW1(byte[] payload) {
        Http2Frame f = Http2Frame.MakeData(1, false, payload);
        return this.WriteFrameW1(f);
    }

    private bool WriteBytesW1(byte[] data) {
        if (data == null) { return false; }
        if (_tls != null) {
            _tls.Write(data, 0, data.Length);
            return true;
        }
        if (_tcp == null) { return false; }
        // 明文路径走 TcpClient 原始字节面（rt_socket_send；显式长度，无 NUL 截断）。
        int sent = 0;
        while (sent < data.Length) {
            int n = _tcp.SendBytes(data, sent, data.Length - sent);
            if (n <= 0) { return false; }
            sent = sent + n;
        }
        return true;
    }

    private Http2Frame ReadFrameW1() {
        byte[] hdr = this.ReadExactW1(9);
        if (hdr == null) { return null; }
        int len = (hdr[0] * 65536) + (hdr[1] * 256) + hdr[2];
        if (len > Http2FrameTypes.MaxFrameSize) { return null; }
        byte[] rest = Http2ByteUtils.ZeroBytes(0);
        if (len > 0) {
            rest = this.ReadExactW1(len);
            if (rest == null) { return null; }
        }
        List<byte> raw = new List<byte>();
        int i = 0;
        while (i < 9) {
            raw.Add(hdr[i]);
            i = i + 1;
        }
        i = 0;
        while (i < len) {
            raw.Add(rest[i]);
            i = i + 1;
        }
        return Http2Frame.Decode(raw.ToArray());
    }

    /// <summary>探测是否有可读字节面数据。明文路径（h2c 本地闭环）经
    /// <see cref="TcpClient.Available"/>；TLS 面无法探测明文缓冲，保守返回 true
    /// （W1 over TLS 未由 e2e 泵压，保持阻塞读语义）。</summary>
    private bool DataAvailableW1() {
        if (_tcp != null && _tls == null) { return _tcp.Available > 0; }
        return true;
    }

    private byte[] ReadExactW1(int n) {
        if (n < 0) { return null; }
        List<byte> out_ = new List<byte>();
        byte[] scratch = Http2ByteUtils.ZeroBytes(1024);
        while (out_.Count < n) {
            int want = n - out_.Count;
            if (want > 1024) { want = 1024; }
            int got = 0;
            if (_tls != null) {
                got = _tls.Read(scratch, 0, want);
            } else if (_tcp != null) {
                // 明文路径走 TcpClient 原始字节面（rt_net_recv；显式长度，无 NUL 截断）。
                got = _tcp.ReceiveBytes(scratch, 0, want);
            } else {
                return null;
            }
            if (got <= 0) { return null; }
            int i = 0;
            while (i < got) {
                out_.Add(scratch[i]);
                i = i + 1;
            }
        }
        return out_.ToArray();
    }

    private bool SendPingAckW1(byte[] opaque) {
        Http2Frame pong = Http2Frame.MakePing(opaque, true);
        return this.WriteFrameW1(pong);
    }

    private WebTransportStream FindStream(int sid) {
        int i = 0;
        while (i < _streams.Count) {
            WebTransportStream s = _streams[i];
            if (s.StreamId == sid) { return s; }
            i = i + 1;
        }
        return null;
    }

    private void GrantStreamFlowControl(long sid) {
        byte[] msd = WebTransportCapsule.MakeMaxStreamData(sid, (long)1073741824);
        this.SendConnectDataW1(msd);
    }

    /// <summary>byte 子数组（List 追加式；规避 byte[] 索引写）。</summary>
    private byte[] SubBytes(byte[] data, int start, int n) {
        List<byte> out_ = new List<byte>();
        int i = 0;
        while (i < n) {
            out_.Add(data[start + i]);
            i = i + 1;
        }
        return out_.ToArray();
    }

    /// <summary>解析 https:// / wss:// / ws:// URL → host/port/path。</summary>
    private bool ParseUrl(string url, bool secure, ref string host, ref int port, ref string path) {
        if (url == null) { return false; }
        string rest = "";
        if (secure) {
            if (url.StartsWith("wss://")) { rest = url.Substring(6, url.Length - 6); }
            else if (url.StartsWith("https://")) { rest = url.Substring(8, url.Length - 8); }
            else { return false; }
        } else {
            if (url.StartsWith("ws://")) { rest = url.Substring(5, url.Length - 5); }
            else { return false; }
        }
        int slash = rest.IndexOf("/");
        string authority = "";
        string p = "/";
        if (slash < 0) { authority = rest; }
        else {
            authority = rest.Substring(0, slash);
            p = rest.Substring(slash, rest.Length - slash);
        }
        if (authority == "") { return false; }
        int colon = authority.IndexOf(":");
        if (colon < 0) {
            host = authority;
            port = secure ? 443 : 80;
        } else {
            host = authority.Substring(0, colon);
            string portStr = authority.Substring(colon + 1, authority.Length - colon - 1);
            port = Convert.ToInt32(portStr);
            if (port <= 0) { return false; }
        }
        if (host == "") { return false; }
        if (p == "") { p = "/"; }
        path = p;
        return true;
    }
}
