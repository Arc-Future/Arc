// RFC 042 D6: P2PMessage — P2P 消息协议桩。
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

public class P2PMessage {
    public MessageType Type { get; set; }
    public string Payload { get; set; }
    public int StreamId { get; set; }
    public bool Ack { get; set; }
    public bool StreamEnd { get; set; }
}
