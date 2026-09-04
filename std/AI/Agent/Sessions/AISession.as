// RFC 038 §4.3：AISession 收敛为「薄门面 + 身份持有者」。
// 回合状态机编排 / 消息存储 / 预算 / 事件 / token 汇总全部下沉到内部协作组件
// AITurnRunner（新建），AISession 只持会话身份（sessionId）与本 runner，委托全部公开 API。
// 开发者使用面不变（面向 AISession 门面），内部实现细节不暴露（AITurnRunner 为 internal）。
// 单线程宿主约束：一个会话由单线程宿主驱动（跨会话并发竞态由 AICoordinator 规避，见宿主文档）。
namespace Arc.Agent;

public class AISession : IDisposable {
    private static int _seq;
    private string _sessionId;
    private AITurnRunner _runner;

    public AISession(
        AIChatClient provider,
        AISessionOptions options,
        AIToolStreamHandler toolHandler,
        AIWiki wiki,
        AIToolSandbox sandbox,
        AITakeOverStreamHandler takeOverHandler,
        AIContextEngine contextEngine
    ) {
        AISession._seq = AISession._seq + 1;
        _sessionId = "sess-" + ("" + AISession._seq);
        _runner = new AITurnRunner(provider, options, toolHandler, wiki, sandbox, takeOverHandler, contextEngine, _sessionId);
    }

    public string SessionId { get { return _sessionId; } }
    public List<AIMessage> Transcript { get { return _runner.Transcript; } }
    public AITurnState Turn { get { return _runner.Turn; } }
    /// <summary>预算上限（回合数；0 = 不设限）。</summary>
    public int MaxTurns { get { return _runner.MaxTurns; } }
    /// <summary>预算上限（消息条数；0 = 不设限）。</summary>
    public int MaxMessages { get { return _runner.MaxMessages; } }
    /// <summary>已用回合数。</summary>
    public int TurnsUsed { get { return _runner.TurnsUsed; } }
    /// <summary>已用消息条数。</summary>
    public int MessagesUsed { get { return _runner.MessagesUsed; } }
    /// <summary>剩余回合额度（0 = 已用尽；-1 = 不设限）。</summary>
    public int RemainingTurns { get { return _runner.RemainingTurns; } }
    /// <summary>剩余消息额度（0 = 已用尽；-1 = 不设限）。</summary>
    public int RemainingMessages { get { return _runner.RemainingMessages; } }
    public AISessionOptions Options { get { return _runner.Options; } }
    public AIHumanRequest PendingHuman { get { return _runner.PendingHuman; } }
    internal AIToolStreamState ActiveToolStream { get { return _runner.ActiveToolStream; } }
    public AIWiki Wiki { get { return _runner.Wiki; } }
    public AIHumanOutcome LastHumanOutcome { get { return _runner.LastHumanOutcome; } }
    public AIToolSet Tools { get { return _runner.Tools; } }
    public AICapabilitySet Capabilities { get { return _runner.Capabilities; } }
    public AIToolSandbox Sandbox { get { return _runner.Sandbox; } }
    /// <summary>窗口裁剪次数（RFC 038 可观测；0 = 从未裁剪）。</summary>
    public int TrimCount { get { return _runner.TrimCount; } }
    /// <summary>上下文工程组合根（RFC 038）：开发者经 Context.AddProvider(...) 注入自定义上下文源。</summary>
    public AIContextEngine Context { get { return _runner.Context; } }

    /// <summary>流式文本增量（业务端边生成边显示；非流式不触发，取 reply.Text）。</summary>
    public Action<string> TextDelta { get { return _runner.TextDelta; } set { _runner.TextDelta = value; } }
    /// <summary>工具即将执行（载荷为完整参数的工具调用；执行前触发）。</summary>
    public Action<AIToolCall> ToolInvoked { get { return _runner.ToolInvoked; } set { _runner.ToolInvoked = value; } }
    /// <summary>工具执行完成（载荷为调用 + 结果；执行后触发）。</summary>
    public Action<AIToolCall, AIToolResult> ToolCompleted { get { return _runner.ToolCompleted; } set { _runner.ToolCompleted = value; } }
    /// <summary>每轮 token 用量上报（业务端计费/监控/缓存命中观察）。</summary>
    public Action<AITokenUsage> UsageReported { get { return _runner.UsageReported; } set { _runner.UsageReported = value; } }
    /// <summary>会话决策轨迹（RFC 038 §13：append-only 单一决策轨迹；取代 Harness 独立事件日志）。</summary>
    public List<AIDecisionEvent> DecisionEvents { get { return _runner.DecisionEvents; } }
    /// <summary>决策事件上报订阅（业务端持久化/展示；决策事件面）。</summary>
    public Action<AIDecisionEvent> DecisionEventReported { get { return _runner.DecisionEventReported; } set { _runner.DecisionEventReported = value; } }
    /// <summary>会话累计 token 用量（含缓存命中/写入；Clear 后归零）。</summary>
    public AITokenUsage TotalUsage { get { return _runner.TotalUsage; } }

    public async Task<AIReply> RunAsync(string userText, CancellationToken cancellationToken) {
        return await _runner.RunAsync(userText, cancellationToken);
    }

    public void Clear() {
        _runner.Clear();
    }

    public AISessionSnapshot Snapshot() {
        return _runner.Snapshot();
    }

    public void Restore(AISessionSnapshot snapshot) {
        _runner.Restore(snapshot);
    }

    public AIToolStreamDisposition PumpToolCallStart(AIToolCallStart start, CancellationToken cancellationToken) {
        return _runner.PumpToolCallStart(start, cancellationToken);
    }

    public void PumpToolArgDelta(AIToolArgDelta delta, CancellationToken cancellationToken) {
        _runner.PumpToolArgDelta(delta, cancellationToken);
    }

    public AIToolResult PumpToolCallEnd(AIToolCallEnd end, CancellationToken cancellationToken) {
        return _runner.PumpToolCallEnd(end, cancellationToken);
    }

    public void EnterAwaitingHuman(AIHumanRequest request) {
        _runner.EnterAwaitingHuman(request);
    }

    public Task ApproveAsync(AIToolCall edited, CancellationToken cancellationToken) {
        return _runner.ApproveAsync(edited, cancellationToken);
    }

    public Task RejectAsync(string reason, CancellationToken cancellationToken) {
        return _runner.RejectAsync(reason, cancellationToken);
    }

    public Task ProvideInputAsync(string text, CancellationToken cancellationToken) {
        return _runner.ProvideInputAsync(text, cancellationToken);
    }

    public async Task<AIReply> ResumeAsync(CancellationToken cancellationToken) {
        return await _runner.ResumeAsync(cancellationToken);
    }

    /// <summary>
    /// 追加决策事件（RFC 038 §13 会话决策事件面）。kind 为强类型枚举，落盘/审计面经
    /// <see cref="AIDecisionEventKindCodec"/> 转 wire 串：airfc:created | airfc:revised |
    /// airfc:rejected | checkpoint:green | checkpoint:rollback | work_summary，与 approval 并列。
    /// </summary>
    public void AppendDecisionEvent(AIDecisionEventKind kind, string detail) {
        _runner.AppendDecisionEvent(kind, detail != null ? detail : "", "", 0);
    }

    /// <summary>追加决策事件（带原因；reason 为 null/空视为无原因）。</summary>
    public void AppendDecisionEvent(AIDecisionEventKind kind, string detail, string? reason) {
        string r = reason != null ? reason : "";
        _runner.AppendDecisionEvent(kind, detail != null ? detail : "", r, 0);
    }

    public void Dispose() {
        _runner.Dispose();
    }
}
