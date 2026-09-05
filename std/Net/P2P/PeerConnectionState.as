// PeerConnectionState —— 拆分自 PeerManager.as（一文件一公开类型）。
namespace Arc.Net.P2P;

public enum PeerConnectionState { Disconnected, Connecting, Connected, Disconnecting }
