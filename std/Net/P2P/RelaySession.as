// RelaySession —— 拆分自 RelayServer.as（一文件一公开类型）。
namespace Arc.Net.P2P.Server;

public class RelaySession {
    public RelaySession() { }
    public bool Forward(string target, string data) {
        throw new NotImplementedException("RelaySession.Forward not implemented (P2P deferred).");
    }
    public string Receive(int timeoutMs) {
        throw new NotImplementedException("RelaySession.Receive not implemented (P2P deferred).");
    }
    public void Close() { }
}
