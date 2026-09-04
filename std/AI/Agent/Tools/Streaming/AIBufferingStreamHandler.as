// RFC 038: default Buffer stream handler — multi-slot arg accumulator (no TakeOver dual buffer).
namespace Arc.Agent;

using Arc;
using Arc.Collections;
using Arc.Text;

/// <summary>
/// Default stream disposition = Buffer. Accumulates ToolArgDelta per active tool call
/// (multi-slot: 交错/并发工具调用各自独立缓冲，不相互覆盖).
/// On End returns a marker result with buffered JSON in Content (IsBufferedArgs = true)
/// for the sandbox to Execute; does not invoke AIToolSet itself (Session / AIToolSandbox owns dispatch).
/// </summary>
internal class AIBufferingStreamHandler : AIToolStreamHandler {
    private List<string> _callIds;
    private List<StringBuilder> _argsList;

    public AIBufferingStreamHandler() {
        _callIds = new List<string>();
        _argsList = new List<StringBuilder>();
    }

    public override AIToolStreamDisposition OnToolCallStart(AIToolCallStart start, CancellationToken cancellationToken) {
        string cid = start != null && start.CallId != null ? start.CallId : "";
        _callIds.Add(cid);
        _argsList.Add(new StringBuilder());
        return AIToolStreamDisposition.Buffer;
    }

    public override void OnToolArgDelta(AIToolArgDelta delta, CancellationToken cancellationToken) {
        if (_argsList.Count == 0 || delta == null) {
            return;
        }
        // 按 delta.CallId 定位槽位 append（交错/并发工具调用各自独立缓冲，不错配参数）；
        // CallId 缺失或未匹配时回退到最后槽（单工具调用的兼容路径）。
        int idx = this.FindSlotIndex(delta.CallId);
        if (idx < 0) {
            idx = _argsList.Count - 1;
        }
        StringBuilder args = _argsList[idx];
        if (delta.Delta != null && delta.Delta != "") {
            args.Append(delta.Delta);
        } else if (delta.Name != null && delta.Name != "") {
            args.Append(delta.Name);
        }
    }

    private int FindSlotIndex(string callId) {
        if (callId == null || callId == "") {
            return -1;
        }
        int n = _callIds.Count;
        int i = 0;
        while (i < n) {
            if (_callIds[i] == callId) {
                return i;
            }
            i = i + 1;
        }
        return -1;
    }

    public override AIToolResult OnToolCallEnd(AIToolCallEnd end, CancellationToken cancellationToken) {
        string cid = end != null && end.CallId != null ? end.CallId : "";
        // Match by CallId when provided; otherwise fall back to the most recent active call.
        int idx = -1;
        if (cid != "") {
            int n = _callIds.Count;
            int i = 0;
            while (i < n) {
                if (_callIds[i] == cid) { idx = i; i = n; }
                i = i + 1;
            }
        }
        if (idx < 0) {
            idx = _argsList.Count - 1;
        }
        string json = "";
        if (idx >= 0 && idx < _argsList.Count) {
            json = _argsList[idx].ToString();
            _callIds.RemoveAt(idx);
            _argsList.RemoveAt(idx);
        }
        // IsBufferedArgs = true：仅"参数就绪"标记（Content 承载完整 args JSON），
        // 异步执行由 RunLoop 统一调度——错误语义与流标记分离（见 AIToolResult）。
        AIToolResult r = new AIToolResult(cid, json, false);
        r.IsBufferedArgs = true;
        return r;
    }
}
