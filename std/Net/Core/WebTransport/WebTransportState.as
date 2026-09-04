// RFC 039 W1/W2: Arc.Net.WebTransport — 会话状态枚举（对齐 WebSocketState 形态）。
namespace Arc.Net.WebTransport;

public enum WebTransportState {
    None = 0,
    Connecting = 1,
    Connected = 2,
    Closing = 3,
    Closed = 4,
    Failed = 5,
}
