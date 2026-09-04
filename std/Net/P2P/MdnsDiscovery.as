// RFC 042: MdnsDiscovery — mDNS 局域网发现。
// 诚实：未接线的 Start/Stop 抛 NotImplementedException。
namespace Arc.Net.P2P;

public class MdnsDiscovery : IDiscovery {
    public string ServiceType { get; }
    public MdnsDiscovery(string serviceType) {
        if (serviceType == null || serviceType == "") { serviceType = "_arcp2p._tcp.local"; }
        ServiceType = serviceType;
    }
    public async Task<void> StartAsync(CancellationToken cancellationToken) {
        throw new NotImplementedException("MdnsDiscovery.StartAsync not implemented (P2P deferred).");
    }
    public async Task<void> StopAsync(CancellationToken cancellationToken) {
        throw new NotImplementedException("MdnsDiscovery.StopAsync not implemented (P2P deferred).");
    }
}