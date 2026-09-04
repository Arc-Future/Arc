// RFC 042 M6: TURN 客户端 (RFC 5766) 桩。
namespace Arc.Net.P2P;

public class TurnClient {
    public string AllocateRelay(string server, int port, string cred) { return null; }
    public bool Send(string data, string target) { return false; }
    public string Receive(int timeoutMs) { return null; }
}
