// S2 (RFC 033 §2.4): Arc.Net — 公共类型：帧常量、头部表、响应对象。
//
// 触碰面：RFC 033 §2.2 S2 里程碑（`std/Net/Core/Http/` 内新增 `Http2` 子目录）；
// 异步单一惯用法（§1.4）。帧常量命名对齐 RFC 7540 表 3。

namespace Arc.Net;

using Arc.Collections;

/// <summary>HTTP/2 帧常量（RFC 7540 §6 表 3）。</summary>
/// <remarks>
/// 常数采用 `class + public const int`（与 std/Arc AttributeTargets 同例——本语言
/// `static int X = 常量` 的静态字段初始化器不受支持；const 在编译期折叠）。
/// </remarks>
internal class Http2FrameTypes {
    public const int Data = 0x0;
    public const int Headers = 0x1;
    public const int Priority = 0x2;
    public const int RstStream = 0x3;
    public const int Settings = 0x4;
    public const int PushPromise = 0x5;
    public const int Ping = 0x6;
    public const int GoAway = 0x7;
    public const int WindowUpdate = 0x8;
    public const int Continuation = 0x9;

    public const int FlagEndStream = 0x1;
    public const int FlagAck = 0x1;
    public const int FlagEndHeaders = 0x4;
    public const int FlagPadded = 0x8;
    public const int FlagPriority = 0x20;

    // 帧长上限（RFC 7540 §6.1——不可变，请求/响应均 16384）。
    public const int MaxFrameSize = 16384;

    // SETTINGS 参数标识（RFC 7540 §6.5.2 表 6）。
    public const int SettingsHeaderTableSize = 0x1;
    public const int SettingsEnablePush = 0x2;
    public const int SettingsMaxConcurrentStreams = 0x3;
    public const int SettingsInitialWindowSize = 0x4;
    public const int SettingsMaxFrameSize = 0x5;
    public const int SettingsMaxHeaderListSize = 0x6;

    // 常量帧长（§6.4 / §6.7 / §6.8 / §6.9）。
    public const int SettingsFrameLength = 0; // ACK；非 ACK 为 6 的倍数
    public const int PingFrameLength = 8;
    public const int GoAwayMinLength = 8;
    public const int WindowUpdateFrameLength = 4;
}

/// <summary>单个 HTTP/2 头字段。</summary>
public class Http2Header {
    public string Name;
    public string Value;

    public Http2Header(string name, string value) {
        Name = name;
        Value = value;
    }
}

/// <summary>HTTP/2 头字段列表（顺序保持接收次序；`Get` 不区分大小写）。</summary>
public class Http2HeaderList {
    private List<Http2Header> _items;

    public Http2HeaderList() {
        _items = new List<Http2Header>();
    }

    /// <summary>头部数量。</summary>
    public int Count {
        get { return _items.Count; }
    }

    /// <summary>追加头字段（请求侧构造 / 响应侧解析共用）。</summary>
    public void Add(string name, string value) {
        _items.Add(new Http2Header(name, value));
    }

    /// <summary>按名取值（不区分大小写）；未命中返回空串。</summary>
    public string Get(string name) {
        int i = 0;
        while (i < _items.Count) {
            Http2Header h = _items[i];
            if (SameName(h.Name, name)) { return h.Value; }
            i = i + 1;
        }
        return "";
    }

    /// <summary>第 i 个头字段名。</summary>
    public string GetName(int i) { return _items[i].Name; }

    /// <summary>第 i 个头字段值。</summary>
    public string GetValue(int i) { return _items[i].Value; }

    private static bool SameName(string a, string b) {
        if (a.Length != b.Length) { return false; }
        int i = 0;
        while (i < a.Length) {
            char ca = a[i];
            char cb = b[i];
            if (ca >= 'A' && ca <= 'Z') { ca = (char)(ca + 32); }
            if (cb >= 'A' && cb <= 'Z') { cb = (char)(cb + 32); }
            if (ca != cb) { return false; }
            i = i + 1;
        }
        return true;
    }
}

/// <summary>HTTP/2 请求（GET/HEAD 等无体请求；可选 Body）。传输原语数据载体（跨包消费需 public）。</summary>
public class Http2Request {
    /// <summary>请求方法（如 "GET"）；空串视为 GET。</summary>
    public string Method;

    /// <summary>请求路径（如 "/index.html?a=1"）；空串视为 "/"。</summary>
    public string Path;

    /// <summary>用户头（伪头与 host 由客户端统一构造，不可覆写）。</summary>
    public Http2HeaderList Headers;

    /// <summary>请求体（可选）；空数组 = 无体（HEADERS 即带 END_STREAM）。</summary>
    public byte[] Body;

    /// <summary>构造请求。</summary>
    public Http2Request(string method, string path) {
        Method = method;
        Path = path;
        Headers = new Http2HeaderList();
        Body = Http2ByteUtils.ZeroBytes(0);
    }
}

/// <summary>HTTP/2 响应。HTTP/2 无 reason phrase——`ReasonPhrase` 固定为空串。</summary>
public class Http2Response {
    /// <summary>`:status` 解析出的状态码（如 200）。</summary>
    public int StatusCode;

    /// <summary>响应头（含伪头 `:status`，值即状态码文本）。</summary>
    public Http2HeaderList Headers;

    /// <summary>DATA 载荷的 UTF-8 文本视图；非文本载荷请用 `BodyBytes`。</summary>
    public string Body;

    /// <summary>DATA 载荷原始字节。</summary>
    public byte[] BodyBytes;

    /// <summary>该响应对应请求流是否为「完整往返」的收尾（含 END_STREAM）。</summary>
    public bool EndOfStream;

    /// <summary>传输/协议失败原因；空串表示成功（StatusCode &gt; 0）。</summary>
    public string Failure;

    /// <summary>末尾 HEADERS（trailers：如 gRPC `grpc-status`/`grpc-message`）；无则空表。</summary>
    public Http2HeaderList Trailers;

    public Http2Response() {
        Headers = new Http2HeaderList();
        Body = "";
        BodyBytes = Http2ByteUtils.ZeroBytes(0);
        EndOfStream = false;
        Failure = "";
        Trailers = new Http2HeaderList();
    }
}
