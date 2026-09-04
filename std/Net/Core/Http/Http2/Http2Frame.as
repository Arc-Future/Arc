// S2 (RFC 033 §2.4): Arc.Net — HTTP/2 帧层（RFC 7540 §4/§6）。
//
// 9 字节帧头 + 帧长校验 + 类型分派。SETTINGS/HEADERS/DATA/WINDOW_UPDATE/PING/GOAWAY
// 全部读写；PRIORITY/RST_STREAM 只读忽略（连接层）。CONTINUATION 后置（诚实边界：
// 单帧 HEADERS，块长超 16384 时上层拒绝，见 Http2Connection）。
//
// 纯 Arc 实现。语言缺位运算（§0.1 不得倒逼语言洞）：24/31 位整数组装以乘加分解，
// 掩码以取模（对 2 的幂）仿真。

namespace Arc.Net;

using Arc.Collections;

/// <summary>单个 HTTP/2 帧（9 字节头 + 载荷）。</summary>
///
/// 设计说明：不设 `Length` 字段——本语言对「名为 Length 的对象 int 字段」的读取
/// 会被 codegen 误降级为 `rt_array_length(receiver)`（读对象头前 8 字节，脏值）。
/// 帧长一律以 `Payload.Length`（真数组）为准（框架类构造器均令二者一致）。
internal class Http2Frame {
    public int Type;
    public int Flags;
    public int StreamId;
    public byte[] Payload;

    public Http2Frame() {
        Type = 0;
        Flags = 0;
        StreamId = 0;
        Payload = Http2ByteUtils.ZeroBytes(0);
    }

    // ── 编码 ──

    /// <summary>帧 → 线上字节（9 字节头 + 载荷）。帧长超上限返回 null。</summary>
    internal byte[] Encode() {
        // 语言：`byte[]` 字段直读不支持 .Length/索引（typeck 归约为 Named("byte_arr")），
        // 先拷贝到局部再访问（与 std/Security AesGcm/X509 同例）。数组为引用，写穿共享。
        byte[] payload = Payload;
        int length = payload.Length;
        if (length > Http2FrameTypes.MaxFrameSize) {
            return null;
        }
        List<byte> raw = new List<byte>();
        raw.Add((byte)((length / 65536) % 256));
        raw.Add((byte)((length / 256) % 256));
        raw.Add((byte)(length % 256));
        raw.Add((byte)Type);
        raw.Add((byte)Flags);
        raw.Add((byte)((StreamId / 16777216) % 128));
        raw.Add((byte)((StreamId / 65536) % 256));
        raw.Add((byte)((StreamId / 256) % 256));
        raw.Add((byte)(StreamId % 256));
        int i = 0;
        while (i < length) {
            raw.Add(payload[i]);
            i = i + 1;
        }
        return raw.ToArray();
    }

    // ── 解码 ──

    /// <summary>线上字节（≥9）→ 帧；帧长超上限/不足返回 null（调用方按连接错误处理）。</summary>
    internal static Http2Frame Decode(byte[] raw) {
        if (raw == null || raw.Length < 9) { return null; }
        int len = (raw[0] * 65536) + (raw[1] * 256) + raw[2];
        if (len > Http2FrameTypes.MaxFrameSize) { return null; } // 帧长校验
        if (raw.Length < 9 + len) { return null; }
        Http2Frame f = new Http2Frame();
        f.Type = raw[3];
        f.Flags = raw[4];
        f.StreamId = ((raw[5] % 128) * 16777216) + (raw[6] * 65536) + (raw[7] * 256) + raw[8];
        f.Payload = Http2ByteUtils.ZeroBytes(len);
        byte[] payload = f.Payload;
        int i = 0;
        while (i < len) {
            payload[i] = raw[9 + i];
            i = i + 1;
        }
        return f;
    }

    // ── 帧构造器（客户端常用） ──

    /// <summary>SETTINGS 帧（含 6 字节参数表；ids/vals 等长）。</summary>
    internal static Http2Frame MakeSettings(int[] ids, int[] vals) {
        Http2Frame f = new Http2Frame();
        f.Type = Http2FrameTypes.Settings;
        f.Flags = 0;
        f.StreamId = 0;
        int flen = ids.Length * 6;
        f.Payload = Http2ByteUtils.ZeroBytes(flen);
        byte[] payload = f.Payload;
        int i = 0;
        while (i < ids.Length) {
            int id = ids[i];
            int v = vals[i];
            payload[i * 6] = (byte)((id / 256) % 256);
            payload[i * 6 + 1] = (byte)(id % 256);
            payload[i * 6 + 2] = (byte)((v / 16777216) % 256);
            payload[i * 6 + 3] = (byte)((v / 65536) % 256);
            payload[i * 6 + 4] = (byte)((v / 256) % 256);
            payload[i * 6 + 5] = (byte)(v % 256);
            i = i + 1;
        }
        return f;
    }

    /// <summary>SETTINGS ACK 帧。</summary>
    internal static Http2Frame MakeSettingsAck() {
        Http2Frame f = new Http2Frame();
        f.Type = Http2FrameTypes.Settings;
        f.Flags = Http2FrameTypes.FlagAck;
        f.StreamId = 0;
        f.Payload = Http2ByteUtils.ZeroBytes(0);
        return f;
    }

    /// <summary>HEADERS 帧（单帧；END_HEADERS 恒置，无 PADDED/PRIORITY 扩展）。</summary>
    internal static Http2Frame MakeHeaders(int streamId, bool endStream, byte[] block) {
        Http2Frame f = new Http2Frame();
        f.Type = Http2FrameTypes.Headers;
        f.Flags = Http2FrameTypes.FlagEndHeaders;
        if (endStream) { f.Flags = f.Flags + Http2FrameTypes.FlagEndStream; }
        f.StreamId = streamId;
        f.Payload = block;
        return f;
    }

    /// <summary>DATA 帧。</summary>
    internal static Http2Frame MakeData(int streamId, bool endStream, byte[] payload) {
        Http2Frame f = new Http2Frame();
        f.Type = Http2FrameTypes.Data;
        f.Flags = 0;
        if (endStream) { f.Flags = f.Flags + Http2FrameTypes.FlagEndStream; }
        f.StreamId = streamId;
        f.Payload = payload;
        return f;
    }

    /// <summary>WINDOW_UPDATE 帧。</summary>
    internal static Http2Frame MakeWindowUpdate(int streamId, int increment) {
        Http2Frame f = new Http2Frame();
        f.Type = Http2FrameTypes.WindowUpdate;
        f.Flags = 0;
        f.StreamId = streamId;
        f.Payload = Http2ByteUtils.ZeroBytes(Http2FrameTypes.WindowUpdateFrameLength);
        byte[] payload = f.Payload;
        payload[0] = (byte)((increment / 16777216) % 128);
        payload[1] = (byte)((increment / 65536) % 256);
        payload[2] = (byte)((increment / 256) % 256);
        payload[3] = (byte)(increment % 256);
        return f;
    }

    /// <summary>PING 帧（8 字节 opaque 载荷）。</summary>
    internal static Http2Frame MakePing(byte[] opaque, bool ack) {
        Http2Frame f = new Http2Frame();
        f.Type = Http2FrameTypes.Ping;
        f.Flags = ack ? Http2FrameTypes.FlagAck : 0;
        f.StreamId = 0;
        f.Payload = Http2ByteUtils.ZeroBytes(Http2FrameTypes.PingFrameLength);
        byte[] payload = f.Payload;
        int i = 0;
        while (i < opaque.Length && i < Http2FrameTypes.PingFrameLength) {
            payload[i] = opaque[i];
            i = i + 1;
        }
        return f;
    }

    /// <summary>GOAWAY 帧（lastStreamId + 错误码；debug 文本后置为空）。</summary>
    internal static Http2Frame MakeGoAway(int lastStreamId, int errorCode) {
        Http2Frame f = new Http2Frame();
        f.Type = Http2FrameTypes.GoAway;
        f.Flags = 0;
        f.StreamId = 0;
        f.Payload = Http2ByteUtils.ZeroBytes(Http2FrameTypes.GoAwayMinLength);
        byte[] payload = f.Payload;
        payload[0] = (byte)((lastStreamId / 16777216) % 128);
        payload[1] = (byte)((lastStreamId / 65536) % 256);
        payload[2] = (byte)((lastStreamId / 256) % 256);
        payload[3] = (byte)(lastStreamId % 256);
        payload[4] = (byte)((errorCode / 16777216) % 256);
        payload[5] = (byte)((errorCode / 65536) % 256);
        payload[6] = (byte)((errorCode / 256) % 256);
        payload[7] = (byte)(errorCode % 256);
        return f;
    }
}
