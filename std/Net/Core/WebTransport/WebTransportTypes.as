// RFC 039 W1/W2: Arc.Net.WebTransport — 内部常量。
//
// W1（WebTransport over HTTP/2，draft-ietf-webtrans-http2-15 · RFC 9297 capsule）：
//   - 会话建立在 HTTP/2 extended CONNECT 流上（:protocol = webtransport）；
//   - DATAGRAM / WT_CLOSE_SESSION / WT_STREAM / WT_STREAM_FIN / WT_MAX_DATA /
//     WT_MAX_STREAM_DATA 均为 CONNECT 流上的 capsule；
//   - SETTINGS_ENABLE_CONNECT_PROTOCOL（0x08）与草案版 SETTINGS_WT_ENABLED
//     （0x2b60）须在连接 SETTINGS 交换中协商为 1。
// W2（WebTransport over HTTP/3，draft-ietf-webtrans-http3-16）：
//   - 会话建立在 HTTP/3 extended CONNECT 流上（:protocol = webtransport-h3）；
//   - SETTINGS_H3_DATAGRAM（0x33）与草案版 SETTINGS_WT_ENABLED（0x2c7cf000）
//     在控制流 SETTINGS 中协商；
//   - 数据报 = QUIC DATAGRAM（RFC 9221），负载首字节为 Quarter Stream ID
//     （= 客户端起始双向流 ID / 4），其后为用户数据报载荷；
//   - 单向流流类型 = 0x54（流字节 = 0x54 | Session ID | 用户数据）；
//     双向流信号 = 0x41（首字节 = 0x41 | Session ID | 用户数据）。
namespace Arc.Net.WebTransport;

using Arc.Collections;

/// <summary>capsule 类型（RFC 9297 §4 · draft-ietf-webtrans-http2-15 §4）。</summary>
internal class WebTransportCapsuleTypes {
    public const long Padding = 0x190B4D38;
    public const long ResetStream = 0x190B4D39;
    public const long StopSending = 0x190B4D3A;
    public const long StreamFin = 0x190B4D3B;
    public const long Stream = 0x190B4D3C;
    public const long MaxData = 0x190B4D3D;
    public const long MaxStreamData = 0x190B4D3E;
    public const long MaxStreamsBidi = 0x190B4D3F;
    public const long MaxStreamsUni = 0x190B4D40;
    public const long DataBlocked = 0x190B4D41;
    public const long StreamDataBlocked = 0x190B4D42;
    public const long StreamsBlockedBidi = 0x190B4D43;
    public const long StreamsBlockedUni = 0x190B4D44;
    public const long Datagram = 0x00;
    public const long CloseSession = 0x2843;
    public const long DrainSession = 0x78AE;
}

/// <summary>SETTINGS 标识（RFC 8441 · RFC 9220 · draft-ietf-webtrans-http2-15
/// §3.1 · draft-ietf-webtrans-http3-16 §3.1）。</summary>
internal class WebTransportSettingsIds {
    public const long EnableConnectProtocol = 0x08;
    public const long H3Datagram = 0x33;
    public const long WtEnabledH2 = 0x2B60;
    public const long WtEnabledH3Draft = 0x2C7CF000;
    public const long WtInitialMaxData = 0x2B61;
    public const long WtInitialMaxStreamsUni = 0x2B64;
    public const long WtInitialMaxStreamsBidi = 0x2B65;
}

/// <summary>W2 流类型 / 双向流信号（draft-ietf-webtrans-http3-16 §3.1）。</summary>
internal class WebTransportStreamTypes {
    public const long W2Uni = 0x54;
    public const long W2BidiSignal = 0x41;
}

/// <summary>RFC 9000 §16 varint 编解码（WebTransport 命名空间内自包含）。
///
/// 背景：S4 `Arc.Net.Quic.QuicVarInt` 对 4/8 字节形态存在缺陷——Encode 的 4 字节
/// 分支字节序错误（右移步长写反），Decode 的宽度公式 `b / 64 + 1` 对首字节
/// 128..191 得 3、192..255 得 4，误落入 8 字节分支返回 -1。WebTransport 数据面
/// 必需 4 字节 varint（WT_STREAM 等 capsule 类型 0x190B4D3*、SETTINGS_WT_ENABLED
/// 0x2c7cf000、DrainSession 0x78AE），故在本层自包含实现正确编解码；
/// 不改 S4 冻结面，缺陷如实记录于 039 W1/W2 验收注记。</summary>
internal class WebTransportVarInt {
    internal static int EncodeLength(long value) {
        if (value < 64) { return 1; }
        if (value < 16384) { return 2; }
        if (value < 1073741824) { return 4; }
        return 8;
    }

    internal static void Encode(List<byte> out_, long value) {
        if (value < 0) { return; }
        int len = EncodeLength(value);
        if (len == 1) {
            out_.Add((byte)value);
            return;
        }
        if (len == 2) {
            out_.Add((byte)(64 + value / 256));
            out_.Add((byte)(value % 256));
            return;
        }
        if (len == 4) {
            out_.Add((byte)(128 + value / 16777216));
            out_.Add((byte)((value / 65536) % 256));
            out_.Add((byte)((value / 256) % 256));
            out_.Add((byte)(value % 256));
            return;
        }
        out_.Add((byte)(192 + value / 72057594037927936));
        out_.Add((byte)((value / 281474976710656) % 256));
        out_.Add((byte)((value / 1099511627776) % 256));
        out_.Add((byte)((value / 4294967296) % 256));
        out_.Add((byte)((value / 16777216) % 256));
        out_.Add((byte)((value / 65536) % 256));
        out_.Add((byte)((value / 256) % 256));
        out_.Add((byte)(value % 256));
    }

    /// <summary>从 data[offset] 解码一个 varint；失败返回 -1，len 为实际长度。</summary>
    internal static long Decode(byte[] data, int offset, out int len) {
        len = 0;
        if (data == null || offset >= data.Length) { return -1; }
        int b = data[offset];
        if (b < 64) {
            if (offset + 1 > data.Length) { return -1; }
            len = 1;
            return b;
        }
        if (b < 128) {
            if (offset + 2 > data.Length) { return -1; }
            len = 2;
            return (long)(b - 64) * 256 + data[offset + 1];
        }
        if (b < 192) {
            if (offset + 4 > data.Length) { return -1; }
            len = 4;
            long v = b - 128;
            v = v * 256 + data[offset + 1];
            v = v * 256 + data[offset + 2];
            v = v * 256 + data[offset + 3];
            return v;
        }
        if (offset + 8 > data.Length) { return -1; }
        len = 8;
        long v8 = b - 192;
        int j = 1;
        while (j < 8) {
            v8 = v8 * 256 + data[offset + j];
            j = j + 1;
        }
        return v8;
    }
}

/// <summary>单向 WebTransport 流（W1：WT_STREAM capsule 携带的流；W2：0x54 流）。</summary>
internal class WebTransportStreamState {
    public int StreamId;
    public List<byte> Inbound;
    public bool ReadFinished;
    public bool Closed;

    public WebTransportStreamState(int streamId) {
        StreamId = streamId;
        Inbound = new List<byte>();
        ReadFinished = false;
        Closed = false;
    }
}
