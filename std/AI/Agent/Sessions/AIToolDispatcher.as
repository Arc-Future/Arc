namespace Arc.Agent;
using Arc;
/// <summary>
/// 工具派发器（单一职责）：持有待执行工具调用游标，逐个经 <see cref="AIToolSandbox"/> 执行并前进。
/// 不持有 transcript/预算/回合状态/事件——那些由 <see cref="AISession"/> 门面在拿到结果后统一
/// 落账（追加消息、预算计数、回合状态迁移、触发业务事件）。纯执行者，无业务耦合。
/// </summary>
internal class AIToolDispatcher {
    private AIToolSandbox _sandbox;
    private List<AIToolCall> _calls;
    private int _index;

    public AIToolDispatcher(AIToolSandbox sandbox) {
        _sandbox = sandbox;
        _calls = null;
        _index = 0;
    }

    /// <summary>开始一轮工具派发（从第 0 个调用起）。</summary>
    public void Begin(List<AIToolCall> calls) {
        _calls = calls;
        _index = 0;
    }

    /// <summary>清零游标（外部不再派发）。</summary>
    public void Reset() {
        _calls = null;
        _index = 0;
    }

    /// <summary>是否还有未执行的工具调用。</summary>
    public bool HasPending() {
        return _calls != null && _index < _calls.Count;
    }

    /// <summary>当前待执行调用（无则 null）。</summary>
    public AIToolCall Current {
        get {
            if (_calls == null || _index >= _calls.Count) {
                return null;
            }
            return _calls[_index];
        }
    }

    /// <summary>当前待执行调用的 CallId（无则空串）。</summary>
    public string CurrentCallId() {
        AIToolCall c = this.Current;
        return c != null && c.CallId != null ? c.CallId : "";
    }

    /// <summary>跳过当前调用（HITL 批准后以编辑版调用执行，原地跳过原始调用）。</summary>
    public void SkipCurrent() {
        _index = _index + 1;
    }

    /// <summary>
    /// 执行当前调用并前进游标，返回执行结果（含能力拒绝/错误/取消）。无待执行调用时返回空成功结果。
    /// </summary>
    public async Task<AIToolResult> ExecuteAndAdvanceAsync(CancellationToken cancellationToken) {
        AIToolCall call = this.Current;
        if (call == null) {
            return new AIToolResult("", "", false);
        }
        AIToolResult result = await _sandbox.ExecuteAsync(call, cancellationToken);
        _index = _index + 1;
        return result;
    }
}