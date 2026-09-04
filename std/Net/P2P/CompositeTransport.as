// RFC 042: CompositeTransport 复合传输实现。
namespace Arc.Net.P2P;

using Arc.Collections;

public class CompositeTransport : ITransport {
    private List<ITransport> _transports;
    public CompositeTransport() { _transports = new List<ITransport>(); }
    public void Add(ITransport t) { _transports.Add(t); }
    public int Count { get { return _transports.Count; } }
    public async Task<IConnection> DialAsync(Multiaddr addr, CancellationToken cancellationToken) {
        throw new NotImplementedException("CompositeTransport.DialAsync not implemented (P2P deferred).");
    }
    public async Task<void> ListenAsync(Multiaddr addr, CancellationToken cancellationToken) {
        throw new NotImplementedException("CompositeTransport.ListenAsync not implemented (P2P deferred).");
    }
}
