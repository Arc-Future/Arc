// PeerRecord —— 拆分自 SignedEnvelope.as（一文件一公开类型）。
namespace Arc.Net.P2P;

public class PeerRecord {
    public PeerId PeerId { get; }
    public long Seq { get; }
    public List<Multiaddr> Addresses { get; }

    public PeerRecord(PeerId peerId, long seq, List<Multiaddr> addresses) {
        PeerId = peerId;
        Seq = seq;
        Addresses = addresses;
    }

    public PeerRecord() {
        PeerId = null;
        Seq = 1;
        Addresses = new List<Multiaddr>();
    }
}
