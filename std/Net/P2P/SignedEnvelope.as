// RFC 042: SignedEnvelope + PeerRecord — 可信路由记录桩。
namespace Arc.Net.P2P;

public class SignedEnvelope {
    public string DomainSep { get; }
    public string PayloadType { get; }
    public string Payload { get; }
    public byte[] Signature { get; }
    public PeerId Issuer { get; }

    public SignedEnvelope(string domainSep, string payloadType, string payload, byte[] signature, PeerId issuer) {
        DomainSep = domainSep;
        PayloadType = payloadType;
        Payload = payload;
        Signature = signature;
        Issuer = issuer;
    }

    /// <summary>对域分隔消息（domainSep:payloadType:payload）真实签名。</summary>
    public static SignedEnvelope Create(PeerKey key, string domainSep, string payloadType, string payload) {
        if (key == null) { throw new ArgumentNullException("key"); }
        byte[] sig = key.Sign(domainSep + ":" + payloadType + ":" + payload);
        return new SignedEnvelope(domainSep, payloadType, payload, sig, key.PublicKey);
    }

    /// <summary>用 signer 公钥真实验证域分隔消息上的签名。</summary>
    public bool Verify(PeerKey signer) {
        if (signer == null || this.Signature == null) { return false; }
        return signer.Verify(this.DomainSep + ":" + this.PayloadType + ":" + this.Payload, this.Signature);
    }
}

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