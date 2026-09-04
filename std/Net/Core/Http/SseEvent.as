// RFC 025 / RFC 033：W3C Server-Sent Events 通用事件（HTTP 层协议模型，非 AI 领域）。
namespace Arc.Net;

/// <summary>
/// 单个 SSE 事件（WHATWG event-stream 规范）：空行分隔的事件块，块内
/// event/data/id/retry 字段行构成一个事件。多个 data: 行以 \n 拼接；
/// 仅携带 data 的事件块才派发（注释行与心跳块不产出事件）。
/// </summary>
public class SseEvent {
    /// <summary>event: 字段值（事件类型）；缺省 "message"（规范默认）。</summary>
    public string Name;

    /// <summary>data: 字段值（多个 data: 行以 \n 拼接）。</summary>
    public string Data;

    /// <summary>id: 字段值（Last-Event-ID 契约）；缺省空串。</summary>
    public string Id;

    /// <summary>retry: 字段值（重连间隔毫秒）；-1 = 本事件未携带。</summary>
    public int Retry;

    public SseEvent() {
        this.Name = "message";
        this.Data = "";
        this.Id = "";
        this.Retry = -1;
    }

    public SseEvent(string name, string data, string id, int retry) {
        this.Name = name != null && name != "" ? name : "message";
        this.Data = data != null ? data : "";
        this.Id = id != null ? id : "";
        this.Retry = retry;
    }
}
