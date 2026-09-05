// WebTransportSettingsIds —— 拆分自 WebTransportTypes.as（一文件一公开类型）。
namespace Arc.Net.WebTransport;
using Arc.Collections;

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
