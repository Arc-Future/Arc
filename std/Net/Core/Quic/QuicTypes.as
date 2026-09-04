// S4 (RFC 033 §2.6): Arc.Net.Quic — 报文/帧/流类型常量与流标识语义（RFC 9000）。
//
// 纯 Arc 常量层。流 ID 类型划分（§2.1）与帧类型（§12.4）是 QUIC 消费层的最小
// 语义面；实际传输（握手/重传/流控）由 rt_quic_* ABI 承载（e2e 以 Rust harness
// 直连验证）。常量命名对齐 RFC 9000。

namespace Arc.Net.Quic;

/// <summary>QUIC 流类型判定与常量（RFC 9000 §2.1）。</summary>
public class QuicStreamType {
    /// <summary>客户端发起双向流（id % 4 == 0）。</summary>
    public static bool IsClientBidi(long id) { return id % 4 == 0; }
    /// <summary>服务器发起双向流（id % 4 == 1）。</summary>
    public static bool IsServerBidi(long id) { return id % 4 == 1; }
    /// <summary>客户端发起单向流（id % 4 == 2）。</summary>
    public static bool IsClientUni(long id) { return id % 4 == 2; }
    /// <summary>服务器发起单向流（id % 4 == 3）。</summary>
    public static bool IsServerUni(long id) { return id % 4 == 3; }
    /// <summary>流为单向（id % 4 ∈ {2, 3}）。</summary>
    public static bool IsUni(long id) { return id % 4 >= 2; }
    /// <summary>流为双向（id % 4 ∈ {0, 1}）。</summary>
    public static bool IsBidi(long id) { return id % 4 < 2; }
}

/// <summary>QUIC 帧类型常量（RFC 9000 §12.4 表 3）。</summary>
internal class QuicFrameTypes {
    public const long Padding = 0x00;
    public const long Ping = 0x01;
    public const long Ack = 0x02;
    public const long AckEcn = 0x03;
    public const long ResetStream = 0x04;
    public const long StopSending = 0x05;
    public const long Crypto = 0x06;
    public const long NewToken = 0x07;
    public const long Stream = 0x08;    // 0x08..0x0f（FIN/LEN/OFF 标志位）
    public const long StreamOff = 0x0c;
    public const long StreamFinOff = 0x0d;
    public const long StreamLenOff = 0x0e;
    public const long StreamFinLenOff = 0x0f;
    public const long MaxData = 0x10;
    public const long MaxStreamData = 0x11;
    public const long MaxStreamsBidi = 0x12;
    public const long MaxStreamsUni = 0x13;
    public const long DataBlocked = 0x14;
    public const long StreamDataBlocked = 0x15;
    public const long StreamsBlockedBidi = 0x16;
    public const long StreamsBlockedUni = 0x17;
    public const long NewConnectionId = 0x18;
    public const long RetireConnectionId = 0x19;
    public const long PathChallenge = 0x1a;
    public const long PathResponse = 0x1b;
    public const long ConnectionClose = 0x1c;   // 0x1d = 应用层关闭
    public const long ConnectionCloseApp = 0x1d;
    public const long HandshakeDone = 0x1e;
}

/// <summary>QUIC 报文类型常量（长头，RFC 9000 §17.2）。</summary>
internal class QuicPacketTypes {
    public const long Initial = 0x00;      // 长头 2 位类型域 00
    public const long ZeroRtt = 0x10;      // 01
    public const long Handshake = 0x20;    // 10
    public const long Retry = 0x30;        // 11
}
