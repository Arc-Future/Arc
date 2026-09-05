// QuicFrameTypes —— 拆分自 QuicTypes.as（一文件一公开类型）。
namespace Arc.Net.Quic;

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
