// RFC 042: IPMonitor — IP 地址变更检测桩。
namespace Arc.Net.P2P;

internal class IPMonitor {
    private int _lastPort;

    public string LastIP { get; }
    public int CheckIntervalMs { get; }

    public IPMonitor() {
        LastIP = "";
        _lastPort = 0;
        CheckIntervalMs = 300000;
    }

    public IPMonitor(int checkIntervalMs) {
        if (checkIntervalMs <= 0) { checkIntervalMs = 300000; }
        LastIP = "";
        _lastPort = 0;
        CheckIntervalMs = checkIntervalMs;
    }

    public bool CheckAndUpdate(int listenPort) { return false; }
}