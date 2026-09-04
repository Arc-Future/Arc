// RFC 033 S1: Arc.Net.WebSocket — WebSocket 帧操作码（RFC 6455 §5.2）。
namespace Arc.Net.WebSocket;

/// <summary>WebSocket 帧操作码（RFC 6455 §5.2）。</summary>
public enum WebSocketOpcode {
    Continuation = 0,
    Text = 1,
    Binary = 2,
    Close = 8,
    Ping = 9,
    Pong = 10,
}
