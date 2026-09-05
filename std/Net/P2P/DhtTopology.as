// DhtTopology —— 拆分自 ITopology.as（一文件一公开类型）。
namespace Arc.Net.P2P;

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
