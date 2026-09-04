// RFC 042: DhtDiscovery — Kademlia DHT 查写发现。
// 诚实：未接线的各方法抛 NotImplementedException。
namespace Arc.Net.P2P;

public class DhtDiscovery : IDiscovery {
    private PeerKey _peerKey;
    private List<string> _bootstrapAddrs;

    public DhtDiscovery(PeerKey key, List<string> bootstrap) {
        _peerKey = key;
        _bootstrapAddrs = bootstrap;
    }

    public async Task<void> StartAsync(CancellationToken cancellationToken) {
        throw new NotImplementedException("DhtDiscovery.StartAsync not implemented (P2P deferred).");
    }
    public async Task<void> StopAsync(CancellationToken cancellationToken) {
        throw new NotImplementedException("DhtDiscovery.StopAsync not implemented (P2P deferred).");
    }
    public bool StoreValue(string key, string val) {
        throw new NotImplementedException("DhtDiscovery.StoreValue not implemented (P2P deferred).");
    }
    public string FindValue(string key) {
        throw new NotImplementedException("DhtDiscovery.FindValue not implemented (P2P deferred).");
    }
    public List<Peer> FindNearest(PeerId target, int k) {
        throw new NotImplementedException("DhtDiscovery.FindNearest not implemented (P2P deferred).");
    }
    public void Bootstrap() {
        throw new NotImplementedException("DhtDiscovery.Bootstrap not implemented (P2P deferred).");
    }
}
