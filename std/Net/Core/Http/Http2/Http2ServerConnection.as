// RFC 049 M-B: Arc.Net — HTTP/2 服务端连接（RFC 7540 §3/§4/§6 服务端侧）。
//
// 与客户端 Http2Connection 镜像：接受前置 + SETTINGS 交换 → 逐帧读取 → 流分派。
// 服务端职责：
//   - 校验客户端 preface（24 字节）+ 收客户端 SETTINGS 回 ACK + 发自身 SETTINGS。
//   - ReadRequest：读完整一个请求（HEADERS 解出 :method/:path + DATA 载荷累积至
//     END_STREAM），期间对连接级帧（SETTINGS/PING/GOAWAY/WINDOW_UPDATE）自动应答。
//   - 写侧：SendResponseHeaders（首 HEADERS）/ SendData（DATA 多帧流式）/
//     SendTrailers（末尾 HEADERS·无 END_HEADERS 之外标志·END_STREAM）。
// 供 gRPC 框架（Arc.Net.Grpc）作传输底座消费；纯 Arc 实现，复用 Http2Frame/
// Hpack/Http2Types 既有原语。
//
// 诚实边界（对齐 033 S2 客户端 + RFC 049 §1.4）：
//   - 单连接顺序处理（一次一个活动流）；多连接/并发多流后置。
//   - 帧长 ≤16384 单帧 HEADERS（CONTINUATION 后置）；HPACK 动态表仅解码（编码用静态表 +
//     不索引字面量）；流控最小（不跟踪窗口增量）。
//   - 响应头由调用方（gRPC 框架）构造；本类仅负责帧装配与收发。

namespace Arc.Net;

using Arc.Collections;
using Arc.Text;

/// <summary>服务端单请求（请求头 + 载荷累积至 END_STREAM）。</summary>
public class Http2ServerRequest {
    public int StreamId;
    public string Method;
    public string Path;
    public Http2HeaderList Headers;
    public byte[] Body;
    public bool EndStream;

    public Http2ServerRequest(int streamId) {
        StreamId = streamId;
        Method = "";
        Path = "";
        Headers = new Http2HeaderList();
        Body = Http2ByteUtils.ZeroBytes(0);
        EndStream = false;
    }
}

/// <summary>
/// HTTP/2 服务端连接：接受客户端前置 + SETTINGS 交换 + 请求读取 + 响应/流式/trailers 写出。
/// 同步传输原语（P1 同步路径）；异步面由 gRPC 门面 Task 包裹（对齐 Http2Connection 单一惯用法）。
/// </summary>
public class Http2ServerConnection {
    private TcpClient _tcp;
    private Hpack _hpack;
    private List<byte> _inbox;
    private bool _closed;
    private bool _handshake;

    public Http2ServerConnection(TcpClient client) {
        _tcp = client;
        _hpack = new Hpack();
        _inbox = new List<byte>();
        _closed = false;
        _handshake = false;
    }

    public bool Connected {
        get { return !_closed; }
    }

    /// <summary>校验客户端 preface、收客户端 SETTINGS、发自身 SETTINGS 并回 ACK。成功返回 true。</summary>
    public bool AcceptHandshake() {
        if (_handshake) { return true; }
        if (_tcp == null) { return false; }

        // 1. 读客户端 preface（24 字节）。
        int guard = 0;
        while (_inbox.Count < 24) {
            if (!this.ReadMore()) { this.Close(); return false; }
            guard = guard + 1;
            if (guard > 64) { this.Close(); return false; }
        }
        byte[] preface = Http2ByteUtils.ZeroBytes(24);
        int i = 0;
        while (i < 24) { preface[i] = _inbox[i]; i = i + 1; }
        this.Consume(24);
        if (!this.IsPreface(preface)) { this.Close(); return false; }

        // 2. 发服务端 SETTINGS（MAX_CONCURRENT_STREAMS=1：顺序处理诚实边界）。
        if (!this.SendFrame(Http2Frame.MakeSettings([Http2FrameTypes.SettingsMaxConcurrentStreams], [1]))) {
            this.Close(); return false;
        }

        // 3. 等待客户端 SETTINGS（非 ACK）并回 ACK。
        int g2 = 0;
        while (g2 < 16) {
            Http2Frame f = this.ReadFrame();
            if (f == null) { this.Close(); return false; }
            if (f.StreamId == 0 && f.Type == Http2FrameTypes.Settings && f.Flags % 2 == 0) {
                if (!this.SendFrame(Http2Frame.MakeSettingsAck())) { this.Close(); return false; }
                _handshake = true;
                return true;
            }
            if (f.StreamId == 0) {
                if (!this.DispatchConnectionFrame(f)) { this.Close(); return false; }
            }
            g2 = g2 + 1;
        }
        this.Close();
        return false;
    }

    /// <summary>读完整一个请求（HEADERS + DATA 至 END_STREAM）。连接失败/EOF 返回 null。</summary>
    public Http2ServerRequest ReadRequest() {
        if (!_handshake || _closed) { return null; }
        // 首帧须为 HEADERS（请求头）。
        Http2Frame first = this.ReadFrame();
        if (first == null) { return null; }
        if (first.StreamId == 0) {
            if (!this.DispatchConnectionFrame(first)) { this.Close(); return null; }
            return this.ReadRequest(); // 连接级帧后继续
        }
        if (first.Type != Http2FrameTypes.Headers) {
            this.Close(); return null;
        }
        int streamId = first.StreamId;
        Http2ServerRequest req = new Http2ServerRequest(streamId);
        Http2HeaderList hs = req.Headers;
        if (!this.DecodeRequestHeaders(first, hs)) { this.Close(); return null; }
        req.Method = hs.Get(":method");
        req.Path = hs.Get(":path");
        req.EndStream = first.Flags % 2 == 1;

        // 累积 DATA 直至 END_STREAM。
        List<byte> body = new List<byte>();
        while (!req.EndStream) {
            Http2Frame f = this.ReadFrame();
            if (f == null) { this.Close(); return null; }
            if (f.StreamId == 0) {
                if (!this.DispatchConnectionFrame(f)) { this.Close(); return null; }
                continue;
            }
            if (f.StreamId != streamId) { this.Close(); return null; } // 顺序处理边界
            if (f.Type == Http2FrameTypes.Data) {
                byte[] payload = f.Payload;
                int i = 0;
                while (i < payload.Length) { body.Add(payload[i]); i = i + 1; }
                if (f.Flags % 2 == 1) { req.EndStream = true; }
            } else if (f.Type == Http2FrameTypes.Headers) {
                // 请求侧 trailers（客户端流式收尾）——合并进头表（gRPC 客户端不常用）。
                if (!this.DecodeRequestHeaders(f, hs)) { this.Close(); return null; }
                if (f.Flags % 2 == 1) { req.EndStream = true; }
            } else if (f.Type == Http2FrameTypes.RstStream) {
                this.Close(); return null;
            } else {
                // WINDOW_UPDATE / PING / 忽略。
            }
        }
        req.Body = body.ToArray();
        return req;
    }

    /// <summary>写响应首 HEADERS（:status 等；不含 END_STREAM）。</summary>
    public bool SendResponseHeaders(int streamId, Http2HeaderList headers) {
        byte[] block = _hpack.EncodeHeaders(headers);
        if (block == null) { return false; }
        return this.SendFrame(Http2Frame.MakeHeaders(streamId, false, block));
    }

    /// <summary>写 DATA 帧（流式多帧；endStream=true 表示该流数据结束）。</summary>
    public bool SendData(int streamId, byte[] data, bool endStream) {
        if (data == null) { data = Http2ByteUtils.ZeroBytes(0); }
        return this.SendFrame(Http2Frame.MakeData(streamId, endStream, data));
    }

    /// <summary>写末尾 trailers HEADERS（grpc-status 等；恒带 END_STREAM）。</summary>
    public bool SendTrailers(int streamId, Http2HeaderList trailers) {
        byte[] block = _hpack.EncodeHeaders(trailers);
        if (block == null) { return false; }
        return this.SendFrame(Http2Frame.MakeHeaders(streamId, true, block));
    }

    /// <summary>优雅关闭：GOAWAY 后关闭 TCP。</summary>
    public void CloseGraceful(int lastStreamId) {
        if (_closed) { return; }
        this.SendFrame(Http2Frame.MakeGoAway(lastStreamId, 0));
        this.Close();
    }

    /// <summary>硬关闭。</summary>
    public void Close() {
        if (_tcp != null) {
            _tcp.Close();
            _tcp = null;
        }
        _closed = true;
    }

    // ── 请求头解码 ──

    private bool DecodeRequestHeaders(Http2Frame f, Http2HeaderList hs) {
        bool endHeaders = (f.Flags / 4) % 2 == 1;
        if (!endHeaders) { return false; } // CONTINUATION 后置
        byte[] payload = f.Payload;
        return _hpack.DecodeHeaders(payload, hs);
    }

    // ── 连接级帧应答 ──

    private bool DispatchConnectionFrame(Http2Frame f) {
        if (f.Type == Http2FrameTypes.Settings && f.Flags % 2 == 0) {
            return this.SendFrame(Http2Frame.MakeSettingsAck());
        }
        if (f.Type == Http2FrameTypes.Ping && f.Flags % 2 == 0) {
            byte[] payload = f.Payload;
            return this.SendFrame(Http2Frame.MakePing(payload, true));
        }
        if (f.Type == Http2FrameTypes.GoAway) {
            this.Close();
            return false;
        }
        return true; // WINDOW_UPDATE / PRIORITY / 未知：忽略
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

    /// <summary>读完整一帧（先攒足 9 字节头，再按帧长攒足载荷）。失败返回 null。</summary>
    private Http2Frame ReadFrame() {
        int guard = 0;
        while (_inbox.Count < 9) {
            if (!this.ReadMore()) { return null; }
            guard = guard + 1;
            if (guard > 64) { return null; }
        }
        int len = (_inbox[0] * 65536) + (_inbox[1] * 256) + _inbox[2];
        if (len > Http2FrameTypes.MaxFrameSize) { return null; }
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
        this.Consume(9 + len);
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

    private void Consume(int n) {
        List<byte> rest = new List<byte>();
        int j = n;
        while (j < _inbox.Count) {
            rest.Add(_inbox[j]);
            j = j + 1;
        }
        _inbox = rest;
    }

    // ── 工具 ──

    private bool IsPreface(byte[] b) {
        // "PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n"
        int[] expect = [
            0x50, 0x52, 0x49, 0x20, 0x2A, 0x20, 0x48, 0x54, 0x54, 0x50, 0x2F, 0x32,
            0x2E, 0x30, 0x0D, 0x0A, 0x0D, 0x0A, 0x53, 0x4D, 0x0D, 0x0A, 0x0D, 0x0A
        ];
        int i = 0;
        while (i < 24) {
            if ((int)b[i] != expect[i]) { return false; }
            i = i + 1;
        }
        return true;
    }
}
