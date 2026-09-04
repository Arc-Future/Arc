// RFC 042: P2PNode — P2P 节点骨架。
// 诚实：PeerId 来自 LocalKey.PublicKey；禁硬编码 "pubkey" 假面。联网能力后置。
namespace Arc.Net.P2P;
using Arc.Net;

public class P2PNode {
    public PeerKey LocalKey { get; }

    public static P2PNode Create(PeerKey localKey) {
        return new P2PNode(localKey);
    }

    public P2PNode(PeerKey localKey) {
        LocalKey = localKey;
    }

    public PeerId PeerId {
        get {
            if (LocalKey == null) {
                return null;
            }
            return LocalKey.PublicKey;
        }
    }
}