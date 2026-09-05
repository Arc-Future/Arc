// RFC 042: AutoNat — 自动 NAT 检测桩。
namespace Arc.Net.P2P;

public class AutoNat {
    public NatStatus Status { get; }

    public AutoNat() {
        Status = NatStatus.Unknown;
    }

    public AutoNat(int requiredSuccesses, int maxAttempts) {
        Status = NatStatus.Unknown;
    }

    public bool IsPublic { get { return Status == NatStatus.Public; } }
}
