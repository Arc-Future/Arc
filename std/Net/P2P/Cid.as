// RFC 042: Cid — 内容标识符桩。
namespace Arc.Net.P2P;

public class Cid {
    public string HashValue { get; set; }

    public Cid() { HashValue = ""; }

    public static Cid FromBytes(string data, MultihashAlgorithm algo) {
        Cid cid = new Cid();
        if (data != null && data.Length() > 0) {
            cid.HashValue = data;
        }
        return cid;
    }

    public static Cid FromBytes(string data, MultihashAlgorithm algo, MulticodecType codec) {
        Cid cid = Cid.FromBytes(data, algo);
        return cid;
    }

    public static Cid Parse(string cidString) {
        Cid cid = new Cid();
        cid.HashValue = cidString;
        return cid;
    }

    public override string ToString() {
        return HashValue;
    }

    public MulticodecType Codec { get; } = MulticodecType.Raw;
    public MultihashAlgorithm HashAlgorithm { get; } = MultihashAlgorithm.SHA256;
}
