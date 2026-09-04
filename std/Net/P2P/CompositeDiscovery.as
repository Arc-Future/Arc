// RFC 042: CompositeDiscovery — 组合发现（多机制聚合）。
// 诚实：仅登记列表可证伪；未接线的方法抛 NotImplementedException。
namespace Arc.Net.P2P;

public class CompositeDiscovery : IDiscovery {
    private List<IDiscovery> _discoveries;
    public CompositeDiscovery() { _discoveries = new List<IDiscovery>(); }
    public void Add(IDiscovery d) { _discoveries.Add(d); }
    public int Count { get { return _discoveries.Count; } }
    public void Start() {
        throw new NotImplementedException("CompositeDiscovery.Start not implemented (P2P deferred).");
    }
    public void Stop() {
        throw new NotImplementedException("CompositeDiscovery.Stop not implemented (P2P deferred).");
    }
    public async Task<void> StartAsync(CancellationToken cancellationToken) {
        throw new NotImplementedException("CompositeDiscovery.StartAsync not implemented (P2P deferred).");
    }
    public async Task<void> StopAsync(CancellationToken cancellationToken) {
        throw new NotImplementedException("CompositeDiscovery.StopAsync not implemented (P2P deferred).");
    }
}
