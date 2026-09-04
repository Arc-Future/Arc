// S2 (RFC 033 §2.4): Arc.Net — h2c 连接（prior knowledge）+ 流复用。
//
// 纯 Arc 实现（编译器核心零领域能力）。基于 TcpClient 原始字节面（SendBytes/
// ReceiveBytes，显式长度、无 NUL 截断）。异步单一惯用法（§1.4）：公共面为
// Task&lt;T&gt;，实现在 P1 同步传输原语上以 Task.FromResult 包裹
// （与 S1 WebSocketClient / P2P TcpTransport.DialAsync 同例，见文件头注释）。
//
// 诚实边界：
//   - prior knowledge 形态（`:method/:scheme/:path/:authority` + SETTINGS 交换），
//     非 Upgrade；h2c 无 TLS（wss/TLS 归 S3/S0）。
//   - 流复用：单个 TCP 连接承载多流；HEADERS 单帧（CONTINUATION 后置——块长超
//     16384 时上层拒绝）；END_STREAM 语义；`:status` 解析；DATA 载荷到 byte[]。
//   - 流控最小：客户端不跟踪发送窗口增量；SETTINGS 窗口/最大帧长取默认
//     (65535/16384)；对端 WINDOW_UPDATE 忽略、PING 自动应答、SETTINGS 自动 ACK。
//   - 动态表仅解码（HPACK 编码不写动态表）；Huffman 仅解码。

namespace Arc.Net;

using Arc.Collections;
using Arc.Text;

/// <summary>单流状态（响应头/体累积）。</summary>
internal class Http2StreamState {
    public int StreamId;
    public Http2HeaderList Headers;
    public List<byte> Body;
    public bool EndStream;
    public bool Failed;
    public int StatusCode;
    public Http2HeaderList Trailers;

    public Http2StreamState(int streamId) {
        StreamId = streamId;
        Headers = new Http2HeaderList();
        Body = new List<byte>();
        EndStream = false;
        Failed = false;
        StatusCode = 0;
        Trailers = new Http2HeaderList();
    }
}

/// <summary>
/// HTTP/2 连接：前置 + SETTINGS 交换 + 帧收发 + 流分派。
/// 结构收敛（RFC 033 §1.0 ①）：本类为**统一门面内部连接层**——SocketsHttpHandler
/// 按 HttpVersionPolicy 路由到本连接（物理收敛，§1.2.i），不再经过渡态
/// Http2Client 公开入口。同步传输原语（P1 同步路径）；异步面由门面 Task 包裹。
/// </summary>
public class Http2Connection {
    private TcpClient _tcp;
    private Hpack _hpack;
    private bool _connected;
    private int _nextStreamId;
    private int _lastStreamId;
    private Dictionary<int, Http2StreamState> _streams;
    private List<byte> _inbox;
    private bool _goAway;
    private string _host;
    private int _port;

    public Http2Connection() {
        _tcp = null;
        _hpack = new Hpack();
        _connected = false;
        _nextStreamId = 1;
        _lastStreamId = 0;
        _streams = new Dictionary<int, Http2StreamState>();
        _inbox = new List<byte>();
        _goAway = false;
        _host = "";
        _port = 0;
    }

    public bool Connected {
        get { return _connected; }
    }

    /// <summary>prior knowledge 建立 h2c 连接：连接 + preface + SETTINGS 交换。</summary>
    public bool Connect(string host, int port) {
        if (_connected) { return true; }
        var cl = new TcpClient();
        cl.SetReceiveTimeout(15000);
        cl.SetSendTimeout(15000);
        cl.SetNoDelay(true);
        if (!cl.Connect(host, port)) { return false; }
        _tcp = cl;
        _connected = true;
        _host = host;
        _port = port;

        // 1. HTTP/2 preface（24 字节）。
        byte[] preface = Encoding.GetBytes("PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n");
        if (!this.SendRaw(preface)) { this.Close(); return false; }

        // 2. 客户端 SETTINGS（ENABLE_PUSH=0：不接收服务器推送）。
        Http2Frame settings = Http2Frame.MakeSettings(
            [Http2FrameTypes.SettingsEnablePush], [0]);
        if (!this.SendFrame(settings)) { this.Close(); return false; }

        // 3. 等待服务器 SETTINGS（非 ACK）并回 ACK。
        int guard = 0;
        while (guard < 16) {
            Http2Frame f = this.ReadFrame();
            if (f == null) { this.Close(); return false; }
            if (f.StreamId == 0 && f.Type == Http2FrameTypes.Settings && f.Flags % 2 == 0) {
                if (!this.SendFrame(Http2Frame.MakeSettingsAck())) { this.Close(); return false; }
                return true;
            }
            if (f.StreamId == 0) {
                if (!this.DispatchConnectionFrame(f)) { this.Close(); return false; }
            }
            guard = guard + 1;
        }
        this.Close();
        return false;
    }

    /// <summary>一次驱动多个请求：全部 HEADERS 先行发送（同连接并发流），随后单接收循环完成各流。</summary>
    internal List<Http2Response> DoRequests(List<Http2Request> requests) {
        int n = requests.Count;
        List<Http2Response> results = new List<Http2Response>();
        int k = 0;
        while (k < n) { results.Add(new Http2Response()); k = k + 1; }
        if (!_connected) {
            k = 0;
            while (k < n) {
                Http2Response cur = results[k];
                cur.Failure = "not connected";
                results[k] = cur;
                k = k + 1;
            }
            return results;
        }
        if (_goAway) {
            k = 0;
            while (k < n) {
                Http2Response cur = results[k];
                cur.Failure = "connection GOAWAY";
                results[k] = cur;
                k = k + 1;
            }
            return results;
        }

        // ── 发送阶段：逐请求开流 + HEADERS（无体时 END_STREAM） ──
        List<int> streamIds = new List<int>();
        k = 0;
        while (k < n) {
            Http2Request req = requests[k];
            int sid = _nextStreamId;
            _nextStreamId = _nextStreamId + 2;
            Http2StreamState st = new Http2StreamState(sid);
            Http2HeaderList hs = this.BuildRequestHeaders(req);
            byte[] block = _hpack.EncodeHeaders(hs);
            if (block == null || block.Length > Http2FrameTypes.MaxFrameSize) {
                Http2Response cur = results[k];
                cur.Failure = "headers block exceeds 16384 (CONTINUATION 后置)";
                results[k] = cur;
                streamIds.Add(-1);
                k = k + 1;
                continue;
            }
            byte[] reqBody = req.Body;
            bool hasBody = reqBody != null && reqBody.Length > 0;
            if (!this.SendFrame(Http2Frame.MakeHeaders(sid, !hasBody, block))) {
                Http2Response cur = results[k];
                cur.Failure = "send HEADERS failed";
                results[k] = cur;
                streamIds.Add(-1);
                k = k + 1;
                continue;
            }
            Dictionary<int, Http2StreamState> streams = _streams;
            streams.Add(sid, st);
            _lastStreamId = sid;
            streamIds.Add(sid);
            if (hasBody) {
                if (!this.SendFrame(Http2Frame.MakeData(sid, true, reqBody))) {
                    Http2Response cur = results[k];
                    cur.Failure = "send DATA failed";
                    results[k] = cur;
                }
            }
            k = k + 1;
        }

        // ── 接收阶段：分派各流帧直至全部完成 ──
        int pending = 0;
        k = 0;
        while (k < streamIds.Count) {
            if (streamIds[k] > 0) { pending = pending + 1; }
            k = k + 1;
        }
        while (pending > 0) {
            Http2Frame f = this.ReadFrame();
            if (f == null) { break; }
            if (f.StreamId == 0) {
                if (!this.DispatchConnectionFrame(f)) { break; }
                continue;
            }
            if (!_streams.ContainsKey(f.StreamId)) { continue; }
            Http2StreamState st = _streams[f.StreamId];
            bool wasDone = st.EndStream || st.Failed;
            this.DispatchStreamFrame(st, f);
            bool nowDone = st.EndStream || st.Failed;
            if (!wasDone && nowDone) { pending = pending - 1; }
        }

        // ── 组装 ──
        k = 0;
        while (k < n) {
            if (streamIds[k] > 0) {
                int sid = streamIds[k];
                Http2Response cur = results[k];
                if (_streams.ContainsKey(sid)) {
                    Http2StreamState st = _streams[sid];
                    if (st.Failed) {
                        cur.Failure = "stream failed (RST/GOAWAY/协议违规)";
                    } else {
                        cur.StatusCode = st.StatusCode;
                        cur.Headers = st.Headers;
                        cur.Trailers = st.Trailers;
                        // 语言缺口（std-only 规避）：`st.Body`（List<byte> 字段）直读后
                        // 直接作为参数传给静态方法会得到空值；先拷到局部（同引用，写穿共享）。
                        List<byte> lst = st.Body;
                        byte[] bodyBytes = lst.ToArray();
                        cur.BodyBytes = bodyBytes;
                        cur.Body = Encoding.GetString(bodyBytes);
                        cur.EndOfStream = st.EndStream;
                    }
                } else {
                    cur.Failure = "stream state missing";
                }
                results[k] = cur;
            }
            k = k + 1;
        }
        // 回收流状态：迭代读与 Remove 分属不同循环，避免 NLL 误报。
        k = 0;
        while (k < n) {
            if (streamIds[k] > 0) {
                _streams.Remove(streamIds[k]);
            }
            k = k + 1;
        }
        return results;
    }

    /// <summary>既有连接上同步发送单请求并返回响应（统一门面内部路由入口）。</summary>
    public Http2Response SendRequest(Http2Request request) {
        List<Http2Request> one = new List<Http2Request>();
        one.Add(request);
        List<Http2Response> rs = this.DoRequests(one);
        if (rs.Count > 0) { return rs[0]; }
        return new Http2Response();
    }

    /// <summary>发送 PING（8 字节 opaque）并等待匹配 PONG；返回是否成功。</summary>
    internal bool SendPing(byte[] opaque) {
        if (!_connected) { return false; }
        if (!this.SendFrame(Http2Frame.MakePing(opaque, false))) { return false; }
        int guard = 0;
        while (guard < 16) {
            Http2Frame f = this.ReadFrame();
            if (f == null) { return false; }
            if (f.StreamId == 0 && f.Type == Http2FrameTypes.Ping && f.Flags % 2 == 1) {
                byte[] payload = f.Payload;
                bool matched = payload.Length == opaque.Length;
                int i = 0;
                while (i < payload.Length && i < opaque.Length && matched) {
                    if (payload[i] != opaque[i]) { matched = false; }
                    i = i + 1;
                }
                if (matched) { return true; }
            }
            if (f.StreamId == 0) {
                if (!this.DispatchConnectionFrame(f)) { return false; }
            }
            guard = guard + 1;
        }
        return false;
    }

    /// <summary>优雅关闭：发送 GOAWAY 后关闭 TCP。</summary>
    internal bool CloseGraceful() {
        if (!_connected) { return true; }
        bool ok = this.SendFrame(Http2Frame.MakeGoAway(_lastStreamId, 0));
        _tcp.Close();
        _connected = false;
        _goAway = true;
        return ok;
    }

    /// <summary>硬关闭。</summary>
    public void Close() {
        if (_tcp != null) {
            _tcp.Close();
            _tcp = null;
        }
        _connected = false;
    }

    // ── 请求头构造 ──

    private Http2HeaderList BuildRequestHeaders(Http2Request req) {
        Http2HeaderList hs = new Http2HeaderList();
        string method = req.Method;
        if (method == null || method == "") { method = "GET"; }
        string path = req.Path;
        if (path == null || path == "") { path = "/"; }
        hs.Add(":method", method);
        hs.Add(":scheme", "http");
        hs.Add(":path", path);
        hs.Add(":authority", _host + ":" + Convert.ToString(_port));
        int i = 0;
        while (i < req.Headers.Count) {
            string name = req.Headers.GetName(i);
            string value = req.Headers.GetValue(i);
            // 伪头/`host` 由 BuildRequestHeaders 统一管理，用户不得覆写。
            if (name == null || name == "") { i = i + 1; continue; }
            bool pseudo = name.Length > 0 && name[0] == ':';
            bool isHost = name.Length == 4
                && (name[0] == 'h' || name[0] == 'H')
                && (name[1] == 'o' || name[1] == 'O')
                && (name[2] == 's' || name[2] == 'S')
                && (name[3] == 't' || name[3] == 'T');
            if (!pseudo && !isHost) {
                hs.Add(name, value);
            }
            i = i + 1;
        }
        return hs;
    }

    // ── 帧分派 ──

    /// <summary>连接级（stream 0）帧：SETTINGS ACK / PING 应答 / GOAWAY 记录。返回是否可继续。</summary>
    private bool DispatchConnectionFrame(Http2Frame f) {
        if (f.Type == Http2FrameTypes.Settings && f.Flags % 2 == 0) {
            return this.SendFrame(Http2Frame.MakeSettingsAck());
        }
        if (f.Type == Http2FrameTypes.Ping && f.Flags % 2 == 0) {
            byte[] payload = f.Payload;
            return this.SendFrame(Http2Frame.MakePing(payload, true));
        }
        if (f.Type == Http2FrameTypes.GoAway) {
            _goAway = true;
            return false;
        }
        return true; // WINDOW_UPDATE 等：忽略
    }

    /// <summary>流级帧分派到对应流状态。</summary>
    private void DispatchStreamFrame(Http2StreamState st, Http2Frame f) {
        if (f.Type == Http2FrameTypes.Headers) {
            byte[] payload = f.Payload;
            bool endHeaders = (f.Flags / 4) % 2 == 1;
            if (!endHeaders) { st.Failed = true; return; } // CONTINUATION 后置
            if (st.Headers.Count > 0) {
                // 首 HEADERS 已解码 → 本 HEADERS 为末尾 trailers（gRPC grpc-status 等）。
                if (!_hpack.DecodeHeaders(payload, st.Trailers)) { st.Failed = true; return; }
                if (f.Flags % 2 == 1) { st.EndStream = true; }
                return;
            }
            // 直接解码进 st.Headers（Http2StreamState 持有、随流存活）。此前用临时
            // `decoded` 承接再逐项 `new Http2Header(decoded.GetName(i), ...)` 浅拷贝
            // 字符串指针——borrow decoded 的字符串；decoded 在 last-use 释放后这些
            // 字符串悬垂 → ParseStatus 读头时 AV（http2_h2c_e2e 既有堆损坏）。
            if (!_hpack.DecodeHeaders(payload, st.Headers)) { st.Failed = true; return; }
            st.StatusCode = this.ParseStatus(st.Headers);
            if (f.Flags % 2 == 1) { st.EndStream = true; }
            return;
        }
        if (f.Type == Http2FrameTypes.Data) {
            byte[] payload = f.Payload;
            List<byte> body = st.Body;
            int i = 0;
            while (i < payload.Length) {
                body.Add(payload[i]);
                i = i + 1;
            }
            if (f.Flags % 2 == 1) { st.EndStream = true; }
            return;
        }
        if (f.Type == Http2FrameTypes.RstStream) {
            st.Failed = true;
            st.EndStream = true;
            return;
        }
        if (f.Type == Http2FrameTypes.PushPromise) {
            st.Failed = true; // 客户端已设 ENABLE_PUSH=0，收到即违规
            return;
        }
        // WINDOW_UPDATE / PRIORITY / 未知：忽略（最小流控）。
    }

    // ── 底层收发 ──

    private bool SendRaw(byte[] data) {
        int sent = 0;
        while (sent < data.Length) {
            int n = _tcp.SendBytes(data, sent, data.Length - sent);
            if (n <= 0) { return false; }
            sent = sent + n;
        }
        return true;
    }

    private bool SendFrame(Http2Frame f) {
        byte[] raw = f.Encode();
        if (raw == null) { return false; }
        return this.SendRaw(raw);
    }

    /// <summary>读完整一帧（先攒足 9 字节头，再按帧长攒足载荷）。返回 null 表示连接失败。</summary>
    private Http2Frame ReadFrame() {
        int guard = 0;
        while (_inbox.Count < 9) {
            if (!this.ReadMore()) { return null; }
            guard = guard + 1;
            if (guard > 64) { return null; }
        }
        int len = (_inbox[0] * 65536) + (_inbox[1] * 256) + _inbox[2];
        if (len > Http2FrameTypes.MaxFrameSize) { return null; } // 帧长校验
        guard = 0;
        while (_inbox.Count < 9 + len) {
            if (!this.ReadMore()) { return null; }
            guard = guard + 1;
            if (guard > 128) { return null; }
        }
        byte[] raw = Http2ByteUtils.ZeroBytes(9 + len);
        int i = 0;
        while (i < 9 + len) {
            raw[i] = _inbox[i];
            i = i + 1;
        }
        // 消费已读字节（重建 inbox = 剩余）。
        List<byte> rest = new List<byte>();
        int j = 9 + len;
        while (j < _inbox.Count) {
            rest.Add(_inbox[j]);
            j = j + 1;
        }
        _inbox = rest;
        return Http2Frame.Decode(raw);
    }

    private bool ReadMore() {
        byte[] temp = Http2ByteUtils.ZeroBytes(32768);
        int n = _tcp.ReceiveBytes(temp, 0, temp.Length);
        if (n <= 0) { return false; }
        int i = 0;
        while (i < n) {
            _inbox.Add(temp[i]);
            i = i + 1;
        }
        return true;
    }

    // ── 工具 ──

    /// <summary>从响应头中解析 `:status` → 状态码；未找到返回 0。</summary>
    private int ParseStatus(Http2HeaderList headers) {
        int i = 0;
        while (i < headers.Count) {
            string nm = headers.GetName(i);
            string vl = headers.GetValue(i);
            if (nm == ":status") {
                return this.ParseInt(headers.GetValue(i));
            }
            i = i + 1;
        }
        return 0;
    }

    private int ParseInt(string s) {
        if (s == null || s == "") { return 0; }
        try {
            return Convert.ToInt32(s, 10);
        } catch {
            return 0;
        }
    }
}
