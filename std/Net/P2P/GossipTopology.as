// GossipTopology —— 拆分自 ITopology.as（一文件一公开类型）。
namespace Arc.Net.P2P;

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
