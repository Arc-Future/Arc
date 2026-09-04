// RFC 042: ConnectionMonitor — 连接健康监控桩。
namespace Arc.Net.P2P;

internal class ConnectionMonitor {
    private int _maxBackoffMs;

    public int CurrentBackoffMs { get; }
    public int FailCount { get; set; }

    public ConnectionMonitor() {
        CurrentBackoffMs = 1000;
        _maxBackoffMs = 60000;
        FailCount = 0;
    }

    public void OnConnectSuccess() { FailCount = 0; }
    public int OnConnectFailure() {
        FailCount = FailCount + 1;
        return CurrentBackoffMs;
    }
}