// RFC 042 D8: ITopology — 拓扑管理。
// FullMesh / Gossip / DHT：可证伪最小选择逻辑（截断 knownPeers），禁止恒空 List 假绿。
namespace Arc.Net.P2P;

public interface ITopology {
    List<PeerId> GetDesiredConnections(List<Peer> knownPeers);
    void OnPeerAdded(Peer peer);
    void OnPeerRemoved(PeerId peerId);
}
