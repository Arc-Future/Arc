// RFC 038: TakeOver handler — owns the only arg buffer (no Session dual copy).
// 应用可注册 FragmentSink 边收边消费增量（低拷贝；如流式写文件），
// End 时返回已消费的完整文本作为工具结果（真实行为，非 marker 占位）。
namespace Arc.Agent;

using Arc;
using Arc.Text;

/// <summary>
/// Returns TakeOver on Start; consumes deltas into this instance's buffer only.
/// FragmentSink 非空时，每个增量同步交付给应用消费（D5/D6：增量面，不整段双缓冲）。
/// On End produces AIToolResult from consumed fragments (incremental path, falsifiable).
/// </summary>
public class AITakeOverStreamHandler : AIToolStreamHandler {
    private StringBuilder _consumed;
    private string _callId;
    public int DeltaCount { get; set; }
    public Action<string> FragmentSink { get; set; }

    public AITakeOverStreamHandler() {
        _consumed = null;
        _callId = "";
        DeltaCount = 0;
        FragmentSink = null;
    }


    public string ConsumedText {
        get {
            if (_consumed == null) {
                return "";
            }
            return _consumed.ToString();
        }
    }

    /// <summary>增量消费回调（应用侧边收边消费；可空）。</summary>

    public override AIToolStreamDisposition OnToolCallStart(AIToolCallStart start, CancellationToken cancellationToken) {
        _callId = start != null && start.CallId != null ? start.CallId : "";
        _consumed = new StringBuilder();
        DeltaCount = 0;
        return AIToolStreamDisposition.TakeOver;
    }

    public override void OnToolArgDelta(AIToolArgDelta delta, CancellationToken cancellationToken) {
        if (_consumed == null || delta == null) {
            return;
        }
        DeltaCount = DeltaCount + 1;
        if (delta.Delta != null && delta.Delta != "") {
            _consumed.Append(delta.Delta);
            if (FragmentSink != null) {
                FragmentSink(delta.Delta);
            }
        }
    }

    public override AIToolResult OnToolCallEnd(AIToolCallEnd end, CancellationToken cancellationToken) {
        string cid = end != null && end.CallId != null ? end.CallId : _callId;
        string body = this.ConsumedText;
        _consumed = null;
        return AIToolResult.Ok(cid, body);
    }
}
