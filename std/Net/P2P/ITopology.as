// RFC 042 D8: ITopology — 拓扑管理。
// FullMesh / Gossip / DHT：可证伪最小选择逻辑（截断 knownPeers），禁止恒空 List 假绿。
namespace Arc.Net.P2P;

public interface ITopology {
    List<PeerId> GetDesiredConnections(List<Peer> knownPeers);
    void OnPeerAdded(Peer peer);
    void OnPeerRemoved(PeerId peerId);
}

public class FullMeshTopology : ITopology {
    private int _maxPeers;
    public FullMeshTopology(int maxPeers) {
        if (maxPeers <= 0) { maxPeers = 20; }
        _maxPeers = maxPeers;
    }
    public List<PeerId> GetDesiredConnections(List<Peer> knownPeers) {
        List<PeerId> result = new List<PeerId>();
        if (knownPeers == null || _maxPeers <= 0) {
            return result;
        }
        int n = knownPeers.Count;
        if (n > _maxPeers) {
            n = _maxPeers;
        }
        for (int i = 0; i < n; i++) {
            Peer p = knownPeers[i];
            if (p != null && p.Id != null) {
                result.Add(p.Id);
            }
        }
        return result;
    }
    public void OnPeerAdded(Peer peer) { }
    public void OnPeerRemoved(PeerId peerId) { }
}

internal class GossipTopology : ITopology {
    private int _fanout;
    public GossipTopology(int fanout) {
        if (fanout <= 0) { fanout = 6; }
        _fanout = fanout;
    }
    public List<PeerId> GetDesiredConnections(List<Peer> knownPeers) {
        List<PeerId> result = new List<PeerId>();
        if (knownPeers == null || _fanout <= 0) {
            return result;
        }
        int n = knownPeers.Count;
        if (n > _fanout) {
            n = _fanout;
        }
        for (int i = 0; i < n; i++) {
            Peer p = knownPeers[i];
            if (p != null && p.Id != null) {
                result.Add(p.Id);
            }
        }
        return result;
    }
    public void OnPeerAdded(Peer peer) { }
    public void OnPeerRemoved(PeerId peerId) { }
}

internal class DhtTopology : ITopology {
    private int _k;
    public DhtTopology(int k) {
        if (k <= 0) { k = 20; }
        _k = k;
    }
    public List<PeerId> GetDesiredConnections(List<Peer> knownPeers) {
        List<PeerId> result = new List<PeerId>();
        if (knownPeers == null || _k <= 0) {
            return result;
        }
        int n = knownPeers.Count;
        if (n > _k) {
            n = _k;
        }
        for (int i = 0; i < n; i++) {
            Peer p = knownPeers[i];
            if (p != null && p.Id != null) {
                result.Add(p.Id);
            }
        }
        return result;
    }
    public void OnPeerAdded(Peer peer) { }
    public void OnPeerRemoved(PeerId peerId) { }
}
