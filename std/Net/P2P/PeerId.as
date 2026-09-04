// RFC 042: PeerId — 对等节点唯一标识。
namespace Arc.Net.P2P;

public class PeerId {
    public string PublicKey { get; }

    public PeerId(string publicKey) {
        PublicKey = publicKey;
    }
}