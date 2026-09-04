// RFC 042: CircuitRelay — 中继穿透（未接线）。
// 诚实：未实现路径抛 NotImplementedException，禁止 return null 假面。
// 同步 throw（非 async 状态机）以保证调用即失败。
namespace Arc.Net.P2P;

public class RelayReservation {
    public Multiaddr RelayAddr { get; }
    public int ExpireAt { get; }
    public RelayReservation(Multiaddr addr, int expireAt) { RelayAddr = addr; ExpireAt = expireAt; }
}

public class CircuitRelay {
    public CircuitRelay() { }

    public Task<RelayReservation> ReserveAsync(Multiaddr relayAddr, CancellationToken cancellationToken = default) {
        throw new NotImplementedException("CircuitRelay.ReserveAsync not implemented (P2P deferred).");
    }

    public Task<IConnection> ConnectViaRelayAsync(PeerId target, RelayReservation reservation, CancellationToken cancellationToken) {
        throw new NotImplementedException("CircuitRelay.ConnectViaRelayAsync not implemented (P2P deferred).");
    }

    public Task<List<Multiaddr>> DiscoverRelaysAsync(CancellationToken cancellationToken) {
        throw new NotImplementedException("CircuitRelay.DiscoverRelaysAsync not implemented (P2P deferred).");
    }
}