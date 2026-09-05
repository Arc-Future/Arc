// Http3SettingsIds —— 拆分自 Http3Types.as（一文件一公开类型）。
namespace Arc.Net;

/// <summary>HTTP/3 SETTINGS 参数（RFC 9114 §7.2.4）。</summary>
internal class Http3SettingsIds {
    public const long QpackMaxTableCapacity = 0x1;
    public const long MaxFieldSectionSize = 0x6;
    public const long QpackBlockedStreams = 0x7;
    public const long EnableConnectProtocol = 0x8;
    public const long H3Datagram = 0x33;
}
