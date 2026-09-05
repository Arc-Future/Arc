// RFC 042 M10: TURN 中继服务器（未接线）。
// 诚实：Start/Forward/Receive 抛 NotImplementedException；IsRunning 恒 false。
namespace Arc.Net.P2P.Server;

public class RelayServer {
    public RelayServer() { }
    public void Start(int port) {
        throw new NotImplementedException("RelayServer.Start not implemented (P2P deferred).");
    }
    public void Stop() { }
    public bool IsRunning { get; }
}
