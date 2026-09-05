// WebTransportStreamTypes —— 拆分自 WebTransportTypes.as（一文件一公开类型）。
namespace Arc.Net.WebTransport;
using Arc.Collections;

/// <summary>W2 流类型 / 双向流信号（draft-ietf-webtrans-http3-16 §3.1）。</summary>
internal class WebTransportStreamTypes {
    public const long W2Uni = 0x54;
    public const long W2BidiSignal = 0x41;
}
