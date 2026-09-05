// Peer —— 拆分自 PeerManager.as（一文件一公开类型）。
namespace Arc.Net.P2P;

public class Peer {
    public PeerId Id { get; set; }
    public List<string> Addresses { get; set; }
    public PeerConnectionState ConnectionState { get; set; }
    public Dictionary<string, string> Metadata { get; set; }
}
