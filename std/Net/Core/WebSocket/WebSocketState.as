// RFC 033 S1: Arc.Net.WebSocket — WebSocket 连接状态。
//
// 对标 C# System.Net.WebSockets.WebSocketState。
namespace Arc.Net.WebSocket;

/// <summary>WebSocket 连接状态。</summary>
public enum WebSocketState {
    Connecting = 0,
    Open = 1,
    Closing = 2,
    Closed = 3,
}
