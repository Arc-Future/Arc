// RFC 042 M3: STUN 客户端 (RFC 5389) 桩。
namespace Arc.Net.P2P;

public class StunClient {
    public StunResult Query(string stunServer, int port) {
        return new StunResult();
    }

    public NatType DetectNatType(string stunServer) {
        return NatType.PortRestrictedCone;
    }
}
