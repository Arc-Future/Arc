// RFC 042: BootstrapDiscovery — 种子地址列表发现。
// 诚实：未接线的 Start/Stop 抛 NotImplementedException；BootstrapCount 可证伪。
namespace Arc.Net.P2P;

public class BootstrapDiscovery : IDiscovery {
    private List<string> _bootstrapAddrs;
    public BootstrapDiscovery(List<string> addrs) { _bootstrapAddrs = addrs; }
    public int BootstrapCount {
        get {
            if (_bootstrapAddrs == null) { return 0; }
            return _bootstrapAddrs.Count;
        }
    }
    public async Task<void> StartAsync(CancellationToken cancellationToken) {
        throw new NotImplementedException("BootstrapDiscovery.StartAsync not implemented (P2P deferred).");
    }
    public async Task<void> StopAsync(CancellationToken cancellationToken) {
        throw new NotImplementedException("BootstrapDiscovery.StopAsync not implemented (P2P deferred).");
    }
}
