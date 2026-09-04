// RFC 042: HeartbeatService — 心跳保活桩。
namespace Arc.Net.P2P;

internal class HeartbeatService {
    public int IntervalMs { get; }
    public int MissedCount { get; set; }

    public HeartbeatService() {
        IntervalMs = 15000;
        MissedCount = 0;
    }

    public HeartbeatService(int intervalMs, int maxMissedPings) {
        if (intervalMs <= 0) { intervalMs = 15000; }
        IntervalMs = intervalMs;
        MissedCount = 0;
    }

    public void Start() { MissedCount = 0; }
    public void Stop() { }
    public void OnPongReceived() { MissedCount = 0; }
}