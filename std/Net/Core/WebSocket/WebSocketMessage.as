// RFC 033 S1: Arc.Net.WebSocket — WebSocket 消息载体。
namespace Arc.Net.WebSocket;

/// <summary>
/// 单帧解码结果（由 <c>WebSocketClient.ReceiveAsync()</c> 返回）。
///
/// 诚实边界：仅承载 ≤125 字节的单帧（RFC 6455 最小面，见
/// <see cref="WebSocketClient"/> 诚实边界）——16-bit/64-bit 扩展长度帧与
/// 含内部 0x00 字节的载荷后置；分片重拼后置（对端发送 Continuation 帧时
/// 以独立消息原样返回）。
/// </summary>
public class WebSocketMessage {
    /// <summary>帧操作码（Text / Binary / Close / Ping / Pong / Continuation）。</summary>
    public WebSocketOpcode Opcode;

    /// <summary>帧载荷。文本/二进制统一以字符串承载；含内部 NUL 的载荷后置。</summary>
    public string Text;

    /// <summary>Close 帧的关闭码（RFC 6455 §7.4，如 1000 = 正常关闭）。</summary>
    public int CloseCode;

    /// <summary>Close 帧的关闭原因。</summary>
    public string CloseReason;

    public WebSocketMessage() {
        this.Opcode = WebSocketOpcode.Text;
        this.Text = "";
        this.CloseCode = 0;
        this.CloseReason = "";
    }

    /// <summary>是否为文本帧。</summary>
    public bool IsText() {
        return this.Opcode == WebSocketOpcode.Text;
    }

    /// <summary>是否为二进制帧。</summary>
    public bool IsBinary() {
        return this.Opcode == WebSocketOpcode.Binary;
    }

    /// <summary>是否为关闭帧。</summary>
    public bool IsClose() {
        return this.Opcode == WebSocketOpcode.Close;
    }
}
