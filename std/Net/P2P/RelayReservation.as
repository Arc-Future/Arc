// RelayReservation —— 拆分自 CircuitRelay.as（一文件一公开类型）。
namespace Arc.Net.P2P;

public class RelayReservation {
    public Multiaddr RelayAddr { get; }
    public int ExpireAt { get; }
    public RelayReservation(Multiaddr addr, int expireAt) { RelayAddr = addr; ExpireAt = expireAt; }
}
