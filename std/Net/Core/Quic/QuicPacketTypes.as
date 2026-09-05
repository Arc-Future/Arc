// QuicPacketTypes —— 拆分自 QuicTypes.as（一文件一公开类型）。
namespace Arc.Net.Quic;

/// <summary>QUIC 报文类型常量（长头，RFC 9000 §17.2）。</summary>
internal class QuicPacketTypes {
    public const long Initial = 0x00;      // 长头 2 位类型域 00
    public const long ZeroRtt = 0x10;      // 01
    public const long Handshake = 0x20;    // 10
    public const long Retry = 0x30;        // 11
}
