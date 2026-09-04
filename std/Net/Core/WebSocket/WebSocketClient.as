// RFC 033 S1 + S3: Arc.Net.WebSocket — WebSocket 客户端（ws:// · wss:// · RFC 6455）。
//
// 对标 C# System.Net.WebSockets.ClientWebSocket 精华（[RFC 003](003-idiom-essence.md)
// 单一惯用法）。基于 TcpClient + NetworkStream 构建；wss:// 在字节层将底层传输
// 桥接为 TlsClientSession（S0 M3 · mbedTLS 内存 BIO 非阻塞握手 · ALPN），
// **帧/掩码/HTTP 升级握手逻辑不重写**（RFC 033 §2.5 S3 触碰面）。
//
// ## 诚实边界（RFC 033 §2.3 / §2.5 权威口径）
//   - ws:// 无 TLS；wss:// 复用 S0 TlsClientSession 做字节层桥接（传输读写经
//     TLS 明文面；ALPN 默认 ["http/1.1"]，RFC 6455 升级基于 HTTP/1.1）。
//   - wss 证书校验最小（对齐 S0 诚实边界）：TrustAnchor 默认 null = 不校验
//     （仅测试面）；设置信任锚后为自签根最小校验；完整链/吊销后置。
//   - permessage-deflate **不协商**（§1.2.d 裁决后置）。
//   - 分片/续帧（fragmentation）：**后置**——对端若发送 Continuation 帧，以独立
//     消息原样返回，不做跨帧重拼（RFC 033 §2.3「分片/续帧最小实现或后置」取后置）。
//   - 帧面最小：仅 ≤125 字节单字节长度帧；16-bit 扩展长度在扩展字节含 0x00 时
//     受底层 string 传输原语 NUL 截断约束而失败（文档标注，非静默错误）；
//     64-bit 扩展长度直接拒绝。
//   - HTTP/1.1 管线化后置；服务器端（HttpListener 等价物）后置——「HttpListener
//     谎言已收口」口径维持，不得复活。
//   - 异步单一惯用法（§1.4 ②）：用户面方法均 Async 后缀 + Task&lt;T&gt;；实现为**真异步**
//     （RFC 028 异步为主）：底层经 TcpClient.SendBytesAsync/ReceiveBytesAsync（Reactor
//     提交 read/write · IOCP/io_uring）与 TlsClientSession.ReadAsync/WriteAsync await，
//     不阻塞调用线程。async Main 自动创建并绑定 Reactor（RFC 038 M2），故
//     ConnectAsync/帧收发/Close 全链路真异步；不再以 Task.FromResult 包裹同步。
//   - 传输载体 = string 面（NUL 终止）：帧字节含 0x00 时按 strlen 截断。客户端
//     掩码帧**重试掩码键直至帧内无 0x00 字节**（RFC 6455 掩码本就随机，重试对端
//     不可见）；接收侧含内部 0x00 的载荷后置。载荷含 0x00 的关闭码（如 0x0100）
//     显式拒绝。
//   - 字节面约束（2026-08-04）：Arc string 为 UTF-8，`"" + (char)b` 对 b ≥ 0x80
//     会经 rt_str_from_codepoint 编码为多字节，破坏 RFC 6455 帧字节布局。帧头/
//     mask key/掩码载荷/关闭码统一经 **StringBuilder.AppendChar 落原始单字节**
//     （rt_text_sb_append_char 写 `value & 0xFF`），载荷字节 1..255 均可精确传输。
//   - 位运算缺失的语言缺口（RFC 033 §0.1「不得倒逼语言洞」）：掩码 XOR 以算术
//     仿真实现（逐位加减），不在本 track 内开语言后门；语言 RFC 另行排期。
namespace Arc.Net.WebSocket;

using Arc;
using Arc.Collections;
using Arc.Net;
using Arc.Net.Security;
using Arc.Security.Cryptography;
using Arc.Text;

/// <summary>
/// WebSocket 客户端——连接 ws:// URL 并完成 RFC 6455 Upgrade 握手，
/// 以掩码帧收发文本消息，自动应答 Ping，完成关闭握手。
/// </summary>
public class WebSocketClient : IDisposable {
    /// <summary>RFC 6455 §1.3 固定 GUID（Sec-WebSocket-Accept 计算）。</summary>
    public const string WS_GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

    // 帧常量（RFC 6455 §5.2；无位运算，直接以整数值表达）。
    private const int OpContinuation = 0;
    private const int OP_TEXT = 1;
    private const int OP_BINARY = 2;
    private const int OP_CLOSE = 8;
    private const int OpPing = 9;
    private const int OP_PONG = 10;
    private const int FinBit = 128;       // 帧首字节 bit7
    private const int MaskBit = 128;      // 帧次字节 bit7

    private TcpClient _client;
    private NetworkStream _stream;
    private TlsClientSession _tls;      // wss://：非 null（S3 · TLS 明文面桥接）
    private WebSocketState _state;
    private string _keyB64;
    private string _serverAccept;
    private bool _secure;               // wss:// 标志
    private string _negotiated;         // wss ALPN 协商结果（"" = 非 TLS）
    private List<string> _appProtocols; // wss ALPN 列表（默认 ["http/1.1"]）
    private X509Certificate2 _trustAnchor; // wss 信任锚（null = 不校验 · 仅测试面）

    /// <summary>创建未连接的 WebSocket 客户端。</summary>
    public WebSocketClient() {
        _state = WebSocketState.Connecting;
        _keyB64 = "";
        _serverAccept = "";
        _negotiated = "";
        _appProtocols = new List<string>();
        _appProtocols.Add("http/1.1");
    }

    /// <summary>当前连接状态。</summary>
    public WebSocketState State {
        get { return _state; }
    }

    /// <summary>本次握手的 Sec-WebSocket-Key（base64；连接失败为空串）。</summary>
    public string SecWebSocketKey {
        get { return _keyB64; }
    }

    /// <summary>握手后从对端收到的 Sec-WebSocket-Accept（可观测/调试）。</summary>
    public string ServerAccept {
        get { return _serverAccept; }
    }

    /// <summary>wss:// 的 ALPN 协议列表（握手前设置；默认 ["http/1.1"]；空 = 不协商）。</summary>
    public List<string> ApplicationProtocols {
        get { return _appProtocols; }
        set { _appProtocols = value; }
    }

    /// <summary>wss:// 的信任锚（自签根，最小校验）；null = 不校验（仅测试面）。</summary>
    public X509Certificate2 TrustAnchor {
        get { return _trustAnchor; }
        set { _trustAnchor = value; }
    }

    /// <summary>wss:// 握手后协商出的 ALPN 协议（"http/1.1"）；ws:// 或未握手为空串。</summary>
    public string NegotiatedApplicationProtocol {
        get { return _negotiated; }
    }

    /// <summary>
    /// 计算 RFC 6455 §4.1 Sec-WebSocket-Accept：
    /// <c>base64(SHA1(key + GUID))</c>。供握手验证与本地测试服务器使用。
    /// </summary>
    /// <param name="key">客户端 Sec-WebSocket-Key（base64 字符串）。</param>
    public static string ComputeAccept(string key) {
        byte[] digest = SHA1.ComputeHash(Encoding.GetBytes(key + WS_GUID));
        return Base64.ToBase64String(digest);
    }

    /// <summary>连接 ws:// 或 wss:// URL 并完成 RFC 6455 Upgrade 握手；成功返回 true。</summary>
    /// <param name="url">ws://host[:port][/path]；wss:// 走 S0 TLS 1.3 会话（字节层桥接）。</param>
    public async Task<bool> ConnectAsync(string url, CancellationToken cancellationToken) {
        return await this.DoConnectAsync(url);
    }

    /// <summary>发送文本帧（客户端掩码）；返回实际写入字节数，失败返回 0。</summary>
    public async Task<int> SendAsync(string message, CancellationToken cancellationToken) {
        return await this.DoSendOpcodeAsync(OP_TEXT, message);
    }

    /// <summary>发送文本帧（SendAsync 的别名，C# ClientWebSocket.SendAsync 形态）。</summary>
    public async Task<int> SendTextAsync(string message, CancellationToken cancellationToken) {
        return await this.DoSendOpcodeAsync(OP_TEXT, message);
    }

    /// <summary>
    /// 接收下一帧（自动应答 Ping 后继续等待数据帧）；连接关闭或协议失败返回 null。
    /// </summary>
    public async Task<WebSocketMessage> ReceiveAsync(CancellationToken cancellationToken) {
        return await this.DoReceiveAsync();
    }

    /// <summary>显式发送 Ping 帧；返回实际写入字节数，失败返回 0。</summary>
    public async Task<int> PingAsync(string data, CancellationToken cancellationToken) {
        return await this.DoSendOpcodeAsync(OpPing, data);
    }

    /// <summary>
    /// 发送 Close 帧并完成关闭握手（等待对端 Close 应答后关闭连接）。
    /// 关闭码 2 字节编码含 0x00 字节时显式失败（NUL 传输约束）。
    /// </summary>
    /// <param name="closeCode">RFC 6455 §7.4 关闭码（如 1000）。</param>
    /// <param name="reason">关闭原因（≤123 字节文本）。</param>
    public async Task<bool> CloseAsync(int closeCode, string reason, CancellationToken cancellationToken) {
        return await this.DoCloseAsync(closeCode, reason);
    }

    /// <summary>硬关闭底层连接（不发送 Close 帧）。</summary>
    public void Close() {
        if (_tls != null) {
            // wss：TlsClientSession.Dispose 释放会话句柄并关闭底层传输（NetworkStream → TcpClient）。
            _tls.Dispose();
            _tls = null;
            _stream = null;
            _client = null;
        } else {
            if (_stream != null) {
                _stream.Close();
                _stream = null;
            }
            if (_client != null) {
                _client.Close();
                _client = null;
            }
        }
        if (_state == WebSocketState.Open || _state == WebSocketState.Closing) {
            _state = WebSocketState.Closed;
        }
    }

    /// <summary>释放资源。</summary>
    public void Dispose() {
        this.Close();
    }

    // ── Private: 连接与握手 ──

    private async Task<bool> DoConnectAsync(string url) {
        if (_state != WebSocketState.Connecting) { return false; }

        string host = "";
        int port = 80;
        string path = "/";
        _secure = false;
        if (!this.ParseWsUrl(url, ref host, ref port, ref path)) { return false; }

        var cl = new TcpClient();
        cl.SetReceiveTimeout(5000);
        cl.SetSendTimeout(5000);
        cl.SetNoDelay(true);
        // ConnectAsync 返回非泛型 Task（rt_socket_connect_async 经 int_result 标记
        // 成败，await 不携带结果），故 await 后以 Connected 属性核实连接状态。
        await cl.ConnectAsync(host, port);
        if (!cl.Connected) { cl.Close(); return false; }
        var ns = new NetworkStream(cl, 5000);
        // 提前绑定 _stream/_client：HTTP 升级握手与帧收发统一经传输桥接助手
        // （WriteRawAsync/ReadRawAsync/ReadLineRawAsync），其 ws 路径按 `_tls == null`
        // 分发到 `_stream` 字段——若仅在连接末尾赋值，握手中 `_stream` 为 null → 空引用崩溃。
        _stream = ns;
        _client = cl;

        // wss://（S3 · RFC 033 §2.5）：底层字节流桥接为 TlsClientSession
        // （S0 M3 · mbedTLS 内存 BIO 非阻塞握手 · ALPN）。仅字节层桥接——
        // 帧/掩码/HTTP 升级握手逻辑不重写，后续收发统一经传输桥接助手。
        if (_secure) {
            var tls = new TlsClientSession(ns);
            tls.TargetHost = host;
            tls.ApplicationProtocols = _appProtocols;
            tls.TrustAnchor = _trustAnchor;
            // wss：真异步握手（RFC 028 异步为主）——TlsClientSession 内部经
            // TcpClient.SendBytesAsync/ReceiveBytesAsync（Reactor）await，不阻塞
            // 调用线程；async Main 已自动创建并绑定 Reactor（RFC 038 M2）。
            try {
                await tls.AuthenticateAsClientAsync();
            } catch (Exception ex) {
                tls.Dispose();
                return false;
            }
            if (!tls.IsAuthenticated) {
                tls.Dispose();
                return false;
            }
            _tls = tls;
            _negotiated = tls.NegotiatedApplicationProtocol;
        }

        // 随机 16 字节 Sec-WebSocket-Key（CSPRNG → hex → base64）。
        string keyHex = CSPRNG.GetBytes(16);
        string keyB64 = HexToBase64(keyHex);
        _keyB64 = keyB64;

        string req = "GET " + path + " HTTP/1.1\r\n"
            + "Host: " + host + ":" + Convert.ToString(port) + "\r\n"
            + "Upgrade: websocket\r\n"
            + "Connection: Upgrade\r\n"
            + "Sec-WebSocket-Key: " + keyB64 + "\r\n"
            + "Sec-WebSocket-Version: 13\r\n"
            + "\r\n";
        if ((await this.WriteRawAsync(req)) <= 0) { this.Close(); return false; }

        // 读状态行与响应头（到空行）。
        string statusLine = await this.ReadLineRawAsync();
        if (statusLine == null || !statusLine.StartsWith("HTTP/1.1 101")) {
            this.Close();
            return false;
        }
        string acceptHeader = "";
        while (true) {
            string hl = await this.ReadLineRawAsync();
            if (hl == null) { this.Close(); return false; }
            if (hl == "") { break; }
            int colon = hl.IndexOf(":");
            if (colon > 0) {
                string name = hl.Substring(0, colon).Trim();
                string value = hl.Substring(colon + 1, hl.Length - colon - 1).Trim();
                if (name.ToLower() == "sec-websocket-accept") { acceptHeader = value; }
            }
        }
        if (acceptHeader == "") { this.Close(); return false; }

        // 验证 Sec-WebSocket-Accept（SHA1(key + GUID) → base64）。
        string expected = ComputeAccept(keyB64);
        _serverAccept = acceptHeader;
        if (acceptHeader != expected) { this.Close(); return false; }

        _state = WebSocketState.Open;
        return true;
    }

    // ── Private: 帧收发 ──

    /// <summary>构造并发送一个掩码数据/控制帧（真异步）；返回写入字节数，失败返回 0。</summary>
    private async Task<int> DoSendOpcodeAsync(int opcode, string payload) {
        // Closing 状态仍允许发送 Close 帧（DoCloseAsync 置 Closing 后再发）。
        if (_state != WebSocketState.Open && !(opcode == OP_CLOSE && _state == WebSocketState.Closing)) {
            return 0;
        }
        int plen = payload.Length;
        if (plen > 125) { return 0; } // 扩展长度后置（NUL 约束 + 最小面）
        int b0 = FinBit + opcode;
        int b1 = MaskBit + plen;

        // 掩码键重试直至帧内无 0x00 字节（string 传输 NUL 截断约束）。
        for (int attempt = 0; attempt < 8; attempt++) {
            string key = this.MakeMaskKey();
            if (key == "") { return 0; }
            string masked = this.MaskPayload(payload, key);
            if (masked == "") { continue; }
            // 帧头字节（b0/b1）恒 ≥ 0x80：`"" + (char)b0` 会经 rt_str_from_codepoint
            // 按 UTF-8 编码为多字节，破坏 RFC 6455 帧字节布局。统一经
            // StringBuilder.AppendChar 以原始单字节写入（mask key / 掩码载荷同理）。
            StringBuilder sb = new StringBuilder();
            sb.Append((char)b0);
            sb.Append((char)b1);
            sb.Append(key);
            sb.Append(masked);
            string frame = sb.ToString();
            return await this.WriteRawAsync(frame);
        }
        return 0;
    }

    /// <summary>接收并解码下一帧（真异步）；连接关闭/协议失败返回 null。</summary>
    private async Task<WebSocketMessage> DoReceiveAsync() {
        while (true) {
            if (_state != WebSocketState.Open && _state != WebSocketState.Closing) {
                return null;
            }
            string hdr = await this.ReadRawAsync(2);
            if (hdr == null || hdr.Length < 2) { this.Close(); return null; }
            int b0 = (int)hdr[0];
            int b1 = (int)hdr[1];
            int opcode = b0 % 16;
            int len = b1 % 128;

            if (len == 126) {
                // 16-bit 扩展长度：扩展字节高位为 0x00 时受 NUL 截断 → 显式失败。
                string ext = await this.ReadRawAsync(2);
                if (ext == null || ext.Length < 2) { this.Close(); return null; }
                len = ((int)ext[0]) * 256 + ((int)ext[1]);
            } else if (len == 127) {
                // 64-bit 扩展长度：后置，直接拒绝。
                this.Close();
                return null;
            }

            bool serverMasked = b1 >= MaskBit;
            string maskKey = "";
            if (serverMasked) {
                maskKey = await this.ReadRawAsync(4);
                if (maskKey == null || maskKey.Length < 4) { this.Close(); return null; }
            }

            string payload = "";
            if (len > 0) {
                payload = await this.ReadRawAsync(len);
                if (payload == null) { this.Close(); return null; }
            }
            if (serverMasked) { payload = this.UnmaskPayload(payload, maskKey); }

            if (opcode == OpPing) {
                // 自动应答 Pong（载荷原样回送）。
                await this.DoSendOpcodeAsync(OP_PONG, payload);
                continue;
            }
            if (opcode == OP_PONG) {
                var m = new WebSocketMessage();
                m.Opcode = WebSocketOpcode.Pong;
                m.Text = payload;
                return m;
            }
            if (opcode == OP_CLOSE) {
                var m = new WebSocketMessage();
                m.Opcode = WebSocketOpcode.Close;
                m.Text = payload;
                if (payload.Length >= 2) {
                    m.CloseCode = ((int)payload[0]) * 256 + ((int)payload[1]);
                    m.CloseReason = payload.Length > 2
                        ? payload.Substring(2, payload.Length - 2) : "";
                } else {
                    m.CloseCode = 1005; // 无状态码（RFC 6455 §7.4.1）
                }
                return m;
            }
            if (opcode == OP_BINARY) {
                var m = new WebSocketMessage();
                m.Opcode = WebSocketOpcode.Binary;
                m.Text = payload;
                return m;
            }
            if (opcode == OpContinuation) {
                // 分片重拼后置：对端发来的 Continuation 帧以独立消息原样返回。
                var m = new WebSocketMessage();
                m.Opcode = WebSocketOpcode.Continuation;
                m.Text = payload;
                return m;
            }
            if (opcode == OP_TEXT) {
                var m = new WebSocketMessage();
                m.Opcode = WebSocketOpcode.Text;
                m.Text = payload;
                return m;
            }
            // 保留操作码：合规服务器不会发送——视为协议违规，关闭连接。
            this.Close();
            return null;
        }
    }

    /// <summary>发送 Close 帧并等待对端 Close 应答（真异步）。</summary>
    private async Task<bool> DoCloseAsync(int closeCode, string reason) {
        if (_state != WebSocketState.Open) { return false; }
        _state = WebSocketState.Closing;

        int hi = (closeCode / 256) % 256;
        int lo = closeCode % 256;
        if (hi == 0 || lo == 0) { this.Close(); return false; } // 编码含 0x00 → 拒绝

        // 关闭码字节可 ≥ 0x80（如 0xE8）：经 StringBuilder.AppendChar 落原始单字节。
        StringBuilder sbp = new StringBuilder();
        sbp.Append((char)hi);
        sbp.Append((char)lo);
        sbp.Append(reason);
        string payload = sbp.ToString();
        if (payload.Length > 125) { this.Close(); return false; }

        if ((await this.DoSendOpcodeAsync(OP_CLOSE, payload)) <= 0) { this.Close(); return false; }

        WebSocketMessage reply = await this.DoReceiveAsync();
        bool ok = reply != null && reply.IsClose();
        this.Close();
        return ok;
    }

    // ── Private: URL 解析 ──

    /// <summary>解析 ws:// 或 wss:// URL 为 host/port/path；其他协议返回 false。</summary>
    /// <remarks>wss://（S3）设 _secure 并默认端口 443；显式端口优先。</remarks>
    private bool ParseWsUrl(string url, ref string host, ref int port, ref string path) {
        if (url == null) { return false; }
        _secure = false;
        string rest = "";
        if (url.StartsWith("wss://")) {
            _secure = true;
            rest = url.Substring(6, url.Length - 6);
        } else if (url.StartsWith("ws://")) {
            rest = url.Substring(5, url.Length - 5);
        } else {
            return false;
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
            port = _secure ? 443 : 80;
        } else {
            host = authority.Substring(0, colon);
            port = this.ParseInt(authority.Substring(colon + 1, authority.Length - colon - 1));
            if (port <= 0) { return false; }
        }
        if (host == "") { return false; }
        if (p == "") { p = "/"; }
        path = p;
        return true;
    }

    // ── Private: 掩码（位运算缺失 → 算术仿真，RFC 033 §0.1 不倒逼语言洞） ──

    /// <summary>字节异或（逐位算术仿真；a,b ∈ [0,255]）。</summary>
    private static int ByteXor(int a, int b) {
        int x = 0;
        int bit = 1;
        int ta = a;
        int tb = b;
        while (ta > 0 || tb > 0) {
            int ba = ta % 2;
            int bb = tb % 2;
            if (ba != bb) { x = x + bit; }
            ta = ta / 2;
            tb = tb / 2;
            bit = bit * 2;
        }
        return x;
    }

    /// <summary>生成 4 字节掩码键（CSPRNG 8 hex 字符 → 4 字节）；含 0x00 时返回空串。
    /// 经 StringBuilder.AppendChar 落原始单字节（`(char)` 拼接对 ≥0x80 按 UTF-8 多字节编码）。</summary>
    private string MakeMaskKey() {
        string hex = CSPRNG.GetBytes(4);
        if (hex == null || hex.Length < 8) { return ""; }
        StringBuilder sb = new StringBuilder();
        for (int i = 0; i < 8; i = i + 2) {
            int hi = HexDigit(hex[i]);
            int lo = HexDigit(hex[i + 1]);
            int b = hi * 16 + lo;
            if (b == 0) { return ""; }
            sb.Append((char)b);
        }
        return sb.ToString();
    }

    /// <summary>对载荷做客户端掩码（逐字节 XOR）；结果含 0x00 时返回空串（调用方重试掩码键）。
    /// 掩码字节经 StringBuilder.AppendChar 落原始单字节，字节值任意（1..255）均可精确传输。</summary>
    private string MaskPayload(string payload, string key) {
        StringBuilder sb = new StringBuilder();
        int n = payload.Length;
        for (int i = 0; i < n; i++) {
            int p = (int)payload[i];
            int k = (int)key[i % 4];
            int m = ByteXor(p, k);
            if (m == 0) { return ""; }
            sb.Append((char)m);
        }
        return sb.ToString();
    }

    /// <summary>对载荷做掩码解码（对端若非法携带掩码位时使用）。</summary>
    private string UnmaskPayload(string payload, string key) {
        string out_ = "";
        int n = payload.Length;
        for (int i = 0; i < n; i++) {
            int p = (int)payload[i];
            int k = (int)key[i % 4];
            out_ = out_ + (char)ByteXor(p, k);
        }
        return out_;
    }

    // ── Private: 传输桥接（S3 · wss 字节层桥接）──
    //
    // WebSocket 帧/掩码/HTTP 升级握手逻辑不重写（RFC 033 §2.5）：帧层在明文面
    // 构造 string（char 即原始字节），由下述助手分发到底层传输（真异步）——
    //   ws://  → NetworkStream（string 面 · ReadAsync/WriteAsync → TcpClient 的
    //            ReceiveAsync/SendAsync → Reactor 真异步）
    //   wss:// → TlsClientSession.ReadAsync/WriteAsync（byte[] 面 · 经
    //            rt_crypto_tls_* + TcpClient.SendBytesAsync/ReceiveBytesAsync，
    //            内部 0x00 不截断；char 按低字节 1:1 落 byte[]，不误用 UTF-8）

    /// <summary>全量写原始字节（帧/握手行；真异步）；返回写入字节数，失败返回 0 或抛传输异常。</summary>
    private async Task<int> WriteRawAsync(string s) {
        if (_tls == null) {
            return await _stream.WriteAsync(s);
        }
        byte[] bytes = this.RawBytes(s);
        await _tls.WriteAsync(bytes, 0, bytes.Length);
        return bytes.Length;
    }

    /// <summary>读取至多 <paramref name="count"/> 字节（真异步）；EOF/失败返回 null 或部分数据。</summary>
    private async Task<string> ReadRawAsync(int count) {
        StringBuilder sb = new StringBuilder();
        if (_tls == null) {
            while (sb.Length < count) {
                string chunk = await _stream.ReadAsync(count - sb.Length);
                if (chunk == null || chunk == "") { break; }
                sb.Append(chunk);
            }
        } else {
            while (sb.Length < count) {
                byte[] buf = ZeroBytes(count - sb.Length);
                int n = await _tls.ReadAsync(buf, 0, buf.Length);
                if (n <= 0) { break; } // EOF（close_notify / 连接关闭）
                int i = 0;
                while (i < n) {
                    sb.Append((char)buf[i]);
                    i = i + 1;
                }
            }
        }
        if (sb.Length == 0) { return null; }
        return sb.ToString();
    }

    /// <summary>读取一行（至 \n，剥离尾部 \r；真异步）；EOF 返回 null。对齐 NetworkStream.ReadLine。</summary>
    private async Task<string> ReadLineRawAsync() {
        StringBuilder sb = new StringBuilder();
        while (true) {
            string c = await this.ReadRawAsync(1);
            if (c == null || c == "") { break; }
            if (c[0] == '\n') { break; }
            sb.Append(c[0]);
        }
        string line = sb.ToString();
        if (line.Length > 0 && line[line.Length - 1] == '\r') {
            line = line.Substring(0, line.Length - 1);
        }
        return line;
    }

    /// <summary>char 串 → 原始单字节 byte[]（低字节 1:1；帧层 char 即字节，勿走 UTF-8）。</summary>
    private byte[] RawBytes(string s) {
        List<byte> buf = new List<byte>();
        int i = 0;
        while (i < s.Length) {
            buf.Add((byte)(int)s[i]);
            i = i + 1;
        }
        return buf.ToArray();
    }

    /// <summary>n 字节零填充数组（语言禁 `new T[expr]` 动态尺寸；同 TlsClientSession 惯例）。</summary>
    private static byte[] ZeroBytes(int n) {
        List<byte> buf = new List<byte>();
        int i = 0;
        while (i < n) {
            buf.Add((byte)0);
            i = i + 1;
        }
        return buf.ToArray();
    }

    // ── Private: 工具 ──

    private int ParseInt(string s) {
        if (s == null || s == "") { return 0; }
        try {
            return Convert.ToInt32(s, 10);
        } catch {
            return -1;
        }
    }
}
