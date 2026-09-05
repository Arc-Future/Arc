// RFC 042: IPeerStore — 对等节点存储。
// InMemoryPeerStore：可证伪内存面（Put/Get/Remove/GetConnectedPeers）；禁空 List 假绿。
// 注意：参数名避免 `record`（RFC 006 关键字）。
namespace Arc.Net.P2P;

public interface IPeerStore {
    bool Put(PeerRecord peerRecord);
    PeerRecord Get(PeerId peerId);
    List<PeerId> GetConnectedPeers();
    void Remove(PeerId peerId);
}
