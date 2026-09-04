// RFC 042: PeerManager — P2P 连接管理器桩。
// 当前 Arc 解析器不支持 += / event / ?. 运算符，使用简化桩实现。
// 当前为协议级可用实现；解析器能力落地后升级为生产级（避免占位承诺）。
namespace Arc.Net.P2P;

public class Peer {
    public PeerId Id { get; set; }
    public List<string> Addresses { get; set; }
    public PeerConnectionState ConnectionState { get; set; }
    public Dictionary<string, string> Metadata { get; set; }
}
public enum PeerConnectionState { Disconnected, Connecting, Connected, Disconnecting }

public class PeerManager {
    private P2PNode _node;
    public PeerManager(PeerKey key) { _node = P2PNode.Create(key); }
    public void Start(int port) { }
    public void Stop() { }
    public void AddDiscovery(IDiscovery discovery) { }
}
