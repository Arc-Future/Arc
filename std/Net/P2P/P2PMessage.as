// RFC 042 D6: P2PMessage — P2P 消息协议桩。
namespace Arc.Net.P2P;

public class P2PMessage {
    public MessageType Type { get; set; }
    public string Payload { get; set; }
    public int StreamId { get; set; }
    public bool Ack { get; set; }
    public bool StreamEnd { get; set; }
}
