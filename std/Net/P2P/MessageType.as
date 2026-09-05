// MessageType —— 拆分自 P2PMessage.as（一文件一公开类型）。
namespace Arc.Net.P2P;

public enum MessageType {
    Ping = 0x0001,
    Pong = 0x0002,
    Data = 0x0100,
    StreamOpen = 0x0200,
    StreamClose = 0x0201,
    StreamData = 0x0202,
    MuxFrame = 0x0300,
    KADRPC = 0x0400,
    Discovery = 0x0401,
}
