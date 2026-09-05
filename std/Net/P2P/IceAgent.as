// RFC 042: IceAgent — ICE 打洞（未接线）。
// 诚实：Gather/GetLocalCandidates 抛 NotImplementedException，禁止空 List 假绿。
namespace Arc.Net.P2P;

public class IceAgent {
    private PeerKey _localKey;

    public IceAgent(PeerKey key) {
        _localKey = key;
    }

    public List<IceCandidate> GetLocalCandidates() {
        throw new NotImplementedException("IceAgent.GetLocalCandidates not implemented (P2P deferred).");
    }

    public void GatherCandidates(string host, int port) {
        throw new NotImplementedException("IceAgent.GatherCandidates not implemented (P2P deferred).");
    }

    public void AddStunServer(string stunServer) {
        throw new NotImplementedException("IceAgent.AddStunServer not implemented (P2P deferred).");
    }

    public void AddTurnServer(string turnServer, string credential) {
        throw new NotImplementedException("IceAgent.AddTurnServer not implemented (P2P deferred).");
    }
}
