namespace Arc.Agent;
using Arc;
/// <summary>
/// 流式工具泵（单一职责）：把 Provider 流式下发的工具事件（Start/ArgDelta/End）路由到
/// 具体 handler（优先 sandbox，次 TakeOver，再 ToolHandler），并记录「待执行」的缓冲调用
/// 与「已完成待回写」的 TakeOver 结果。不持有回合状态——状态迁移由 <see cref="AISession"/>
/// 门面根据本泵返回的处置/结果决定。
/// </summary>
internal class AISessionStreamPump {
    private AIToolSandbox _sandbox;
    private AITakeOverStreamHandler _takeOver;
    private AIToolStreamHandler _toolHandler;
    private AIToolStreamState _activeToolStream;
    private bool _streamToolActivity;
    private List<AIToolCall> _pendingStreamCalls;
    private List<AIToolResult> _pendingTakeOverResults;
    private List<AIToolCall> _pendingTakeOverEcho;

    public AISessionStreamPump(AIToolSandbox sandbox, AITakeOverStreamHandler takeOver, AIToolStreamHandler toolHandler) {
        _sandbox = sandbox;
        _takeOver = takeOver;
        _toolHandler = toolHandler;
        _activeToolStream = null;
        _streamToolActivity = false;
        _pendingStreamCalls = new List<AIToolCall>();
        _pendingTakeOverResults = new List<AIToolResult>();
        _pendingTakeOverEcho = new List<AIToolCall>();
    }

    /// <summary>当前活动工具流（Start 后、End 前非空）。</summary>
    public AIToolStreamState ActiveToolStream { get { return _activeToolStream; } }

    /// <summary>本轮是否出现过流式工具活动（End 置位）。</summary>
    public bool HadStreamToolActivity { get { return _streamToolActivity; } }

    /// <summary>Buffer 路径待执行的完整参数调用。</summary>
    public List<AIToolCall> PendingCalls { get { return _pendingStreamCalls; } }

    /// <summary>TakeOver 已完成、待 assistant 回显之后写回的结果。</summary>
    public List<AIToolResult> PendingTakeOverResults { get { return _pendingTakeOverResults; } }

    /// <summary>TakeOver 已完成调用的回显。</summary>
    public List<AIToolCall> PendingTakeOverEcho { get { return _pendingTakeOverEcho; } }

    /// <summary>清空本轮待执行/待回写状态（回合开始前调用）。</summary>
    public void ResetPending() {
        _pendingStreamCalls = new List<AIToolCall>();
        _pendingTakeOverResults = new List<AIToolResult>();
        _pendingTakeOverEcho = new List<AIToolCall>();
        _streamToolActivity = false;
        _activeToolStream = null;
    }

    /// <summary>取走本轮缓冲待执行调用并清空缓冲（供 RunLoop 统一异步调度）。</summary>
    public List<AIToolCall> TakePendingCalls() {
        List<AIToolCall> calls = _pendingStreamCalls;
        _pendingStreamCalls = new List<AIToolCall>();
        return calls;
    }

    /// <summary>读取并复位「本轮是否出现流式工具活动」标志。</summary>
    public bool ConsumeToolActivity() {
        bool v = _streamToolActivity;
        _streamToolActivity = false;
        return v;
    }

    /// <summary>清空当前活动工具流（回合收尾时调用）。</summary>
    public void ClearActiveStream() {
        _activeToolStream = null;
    }

    /// <summary>清空 TakeOver 待回写缓冲（assistant 回显写回后调用）。</summary>
    public void ClearTakeOver() {
        _pendingTakeOverResults = new List<AIToolResult>();
        _pendingTakeOverEcho = new List<AIToolCall>();
    }

    /// <summary>工具流开始：路由到具体 handler，返回处置（Buffer/TakeOver/Reject）。</summary>
    public AIToolStreamDisposition OnToolCallStart(AIToolCallStart start, CancellationToken cancellationToken) {
        string cid = start != null ? start.CallId : "";
        string name = start != null ? start.ToolName : "";
        _activeToolStream = new AIToolStreamState(cid, name);
        // Prefer concrete sandbox / TakeOver fields — abstract virtual dispatch from the
        // provider stream was observed to skip override bodies (language gap).
        AIToolStreamDisposition d = AIToolStreamDisposition.Buffer;
        bool handled = false;
        if (_sandbox != null) {
            d = _sandbox.OnToolCallStart(start, cancellationToken);
            handled = true;
        } else if (_takeOver != null) {
            d = _takeOver.OnToolCallStart(start, cancellationToken);
            handled = true;
        } else if (_toolHandler != null) {
            d = _toolHandler.OnToolCallStart(start, cancellationToken);
            handled = true;
        }
        if (handled) {
            _activeToolStream.Disposition = d;
            return d;
        }
        return AIToolStreamDisposition.Buffer;
    }

    /// <summary>工具参数增量：路由到具体 handler。</summary>
    public void OnToolArgDelta(AIToolArgDelta delta, CancellationToken cancellationToken) {
        if (_sandbox != null) {
            _sandbox.OnToolArgDelta(delta, cancellationToken);
        } else if (_takeOver != null) {
            _takeOver.OnToolArgDelta(delta, cancellationToken);
        } else if (_toolHandler != null) {
            _toolHandler.OnToolArgDelta(delta, cancellationToken);
        }
    }

    /// <summary>
    /// 工具流结束：路由到具体 handler 取结果，并分类记录——Buffer 路径入待执行
    /// （异步统一调度）；TakeOver/直接结果入待回写（等 assistant 回显先入再写）。
    /// </summary>
    public AIToolResult OnToolCallEnd(AIToolCallEnd end, CancellationToken cancellationToken) {
        _streamToolActivity = true;
        string streamName = _activeToolStream != null ? _activeToolStream.Name : "";
        AIToolResult result = null;
        if (_sandbox != null) {
            result = _sandbox.OnToolCallEnd(end, cancellationToken);
        } else if (_takeOver != null) {
            result = _takeOver.OnToolCallEnd(end, cancellationToken);
        } else if (_toolHandler != null) {
            result = _toolHandler.OnToolCallEnd(end, cancellationToken);
        }
        _activeToolStream = null;
        if (result == null) {
            result = new AIToolResult(end != null ? end.CallId : "", "", false);
        }
        if (result.IsBufferedArgs) {
            // Buffer 路径：仅收集完整 args，异步执行由 RunLoop 统一调度。
            string cid = result.CallId;
            string argsJson = result.Content != null ? result.Content : "";
            _pendingStreamCalls.Add(new AIToolCall(cid, streamName, argsJson));
            return result;
        }
        // TakeOver / 直接 handler 结果：已完成，但需等 assistant tool_calls 回显先入
        // transcript 再写 tool 结果（协议顺序）——延迟到 RunLoop 在 assistant 之后统一写回。
        _pendingTakeOverResults.Add(result);
        _pendingTakeOverEcho.Add(new AIToolCall(result.CallId, streamName, result.Content != null ? result.Content : ""));
        return result;
    }
}