// S4 (RFC 033 §2.6): Arc.Net — 帧/设置/流类型常量（RFC 9114）。
//
// 纯 Arc 常量层，供 Http3Frame / Qpack 使用。常量命名对齐 RFC 9114
// §7.2（帧类型表 1）、§7.2.4（SETTINGS 参数）、§6.2（流类型）。

namespace Arc.Net;

/// <summary>HTTP/3 帧类型（RFC 9114 §7.2 表 1）。</summary>
internal class Http3FrameTypes {
    public const long Data = 0x00;
    public const long Headers = 0x01;
    public const long CancelPush = 0x03;
    public const long Settings = 0x04;
    public const long PushPromise = 0x05;
    public const long GoAway = 0x07;
    public const long MaxPushId = 0x0d;
    public const long PriorityUpdateRequest = 0xf0700;   // RFC 9218（后置）
}

/// <summary>HTTP/3 SETTINGS 参数（RFC 9114 §7.2.4）。</summary>
internal class Http3SettingsIds {
    public const long QpackMaxTableCapacity = 0x1;
    public const long MaxFieldSectionSize = 0x6;
    public const long QpackBlockedStreams = 0x7;
    public const long EnableConnectProtocol = 0x8;
    public const long H3Datagram = 0x33;
}

/// <summary>HTTP/3 单向流类型（RFC 9114 §6.2）。</summary>
internal class Http3StreamTypes {
    public const long Control = 0x00;
    public const long Push = 0x01;
    public const long QpackEncoder = 0x02;
    public const long QpackDecoder = 0x03;
}

/// <summary>HTTP/3 错误码（RFC 9114 §8.1）。</summary>
internal class Http3ErrorCodes {
    public const long NoError = 0x100;
    public const long ProtocolError = 0x101;
    public const long InternalError = 0x102;
    public const long StreamCreationError = 0x103;
    public const long ClosedCriticalStream = 0x104;
    public const long FrameUnexpected = 0x105;
    public const long FrameError = 0x106;
    public const long ExcessLoad = 0x107;
    public const long IdError = 0x108;
    public const long SettingsError = 0x109;
    public const long MissingSettings = 0x10a;
    public const long RequestRejected = 0x10b;
    public const long RequestCanceled = 0x10c;
    public const long RequestIncomplete = 0x10d;
    public const long MessageError = 0x10e;
    public const long ConnectError = 0x10f;
    public const long VersionFallback = 0x110;
}

/// <summary>单个 HTTP/3 头字段（伪头与常规头统一表示）。</summary>
public class Http3Header {
    public string Name;
    public string Value;

    public Http3Header(string name, string value) {
        Name = name;
        Value = value;
    }
}

/// <summary>HTTP/3 头字段列表（顺序保持接收次序；`Get` 不区分大小写）。</summary>
public class Http3HeaderList {
    private List<Http3Header> _items;

    public Http3HeaderList() {
        _items = new List<Http3Header>();
    }

    /// <summary>头部数量。</summary>
    public int Count {
        get { return _items.Count; }
    }

    /// <summary>追加头字段（请求侧构造 / 响应侧解析共用）。</summary>
    public void Add(string name, string value) {
        _items.Add(new Http3Header(name, value));
    }

    /// <summary>按名取值（不区分大小写）；未命中返回空串。</summary>
    public string Get(string name) {
        int i = 0;
        while (i < _items.Count) {
            Http3Header h = _items[i];
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

    /// <summary>字节数组工具（零填充分配；语言禁 `new T[expr]`——与 Http2ByteUtils 同例）。</summary>
    internal class Http3ByteUtils {
        /// <summary>n 字节零填充数组。</summary>
        internal static byte[] ZeroBytes(int n) {
            List<byte> buf = new List<byte>();
            int i = 0;
            while (i < n) {
                buf.Add((byte)0);
                i = i + 1;
            }
            return buf.ToArray();
        }

        /// <summary>从 data[start] 起复制 n 字节（List 追加式拷贝，规避 byte[] 索引写）。</summary>
        internal static byte[] Slice(byte[] data, int start, int n) {
            List<byte> out_ = new List<byte>();
            int i = 0;
            while (i < n) {
                out_.Add(data[start + i]);
                i = i + 1;
            }
            return out_.ToArray();
        }
    }
