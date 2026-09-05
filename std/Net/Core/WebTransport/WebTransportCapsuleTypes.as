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
