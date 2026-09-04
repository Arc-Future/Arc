// RFC 038 §4.3：回合状态机 + 消息存储 + 协作组件编排的单一归属者。
// AITurnRunner 接管 RunLoopAsync / NextModelReplyAsync / DispatchAllAsync / 工具循环护栏 /
// 消息追加 / 预算 / 事件 / token 汇总，并以内部协作组件（AIToolDispatcher / AISessionStreamPump /
// AIWindowManager / AIContextEngine / AIHumanGate）完成各自单一职责。
// AISession 降级为薄门面：只持身份（sessionId）与本 runner，委托全部运行时行为。
namespace Arc.Agent;
using Arc;
using Arc.Collections;

/// <summary>
/// 回合运行器（单一职责）：一个会话回合循环的状态机 + 消息/预算/事件/用量的单一事实源。
/// 不持有「会话身份/门面」——由 <see cref="AISession"/> 门面持有并委托。internal，非开发者 API。
/// 单线程宿主约束：本运行器及其共享沙箱/预算在单线程宿主上运行；共享状态（沙箱 Results/
/// 调用计数）无锁安全，多线程并发驱动同一会话是未定义行为。
/// </summary>
internal class AITurnRunner : IDisposable {
    private AIChatClient _provider;
    private AISessionOptions _options;
    private AIToolStreamHandler _toolHandler;
    private AITakeOverStreamHandler _takeOverHandler;
    private AIWiki _wiki;
    private AIToolSandbox _sandbox;
    private string _sessionId;
    private List<AIMessage> _transcript;
    private List<AIMessage> _requestMessages;
    private AITurnState _turn;
    private AISessionBudget _budget;
    private AIHumanGate _humanGate;
    // 协作组件（RFC 038：单一职责）
    private AIToolDispatcher _dispatcher;
    private AISessionStreamPump _pump;
    private bool _disposed;
    private int _trimCount;
    private string _lastToolSignature;
    private int _consecutiveIdenticalRounds;
    // 业务端事件接收（private 字段 + 属性；规避 public 委托字段 codegen 缺陷 C6）
    private Action<string> _textDelta;
    private Action<AIToolCall> _toolInvoked;
    private Action<AIToolCall, AIToolResult> _toolCompleted;
    private Action<AITokenUsage> _usageReported;
    // 会话决策轨迹（RFC 038 §13 / RFC 043 M5–M6：append-only 单一决策轨迹，取代 Harness 独立事件日志）
    private List<AIDecisionEvent> _decisionEvents;
    private Action<AIDecisionEvent> _decisionEventReported;
    // 会话累计 token 用量（含缓存命中/写入）
    private int _totalPromptTokens;
    private int _totalCompletionTokens;
    private int _totalCacheReadTokens;
    private int _totalCacheCreationTokens;
    private AIContextEngine _contextEngine;
    private AIContextSession _contextSession;
    private string _lastUserText;
    // 本回合是否已执行过工具（RFC 038：工具回合后的文本可能是中间进度文本，
    // 需续回合到最终文本；纯聊天（无工具）的文本才立即终结）。
    private bool _sawToolThisTurn;

    public AITurnRunner(
        AIChatClient provider,
        AISessionOptions options,
        AIToolStreamHandler toolHandler,
        AIWiki wiki,
        AIToolSandbox sandbox,
        AITakeOverStreamHandler takeOverHandler,
        AIContextEngine contextEngine,
        string sessionId
    ) {
        if (provider == null) { throw new ArgumentNullException("provider"); }
        if (options == null) { throw new ArgumentNullException("options"); }
        _provider = provider;
        _options = options;
        _toolHandler = toolHandler;
        _takeOverHandler = takeOverHandler;
        _wiki = wiki;
        _sandbox = sandbox;
        _sessionId = sessionId != null ? sessionId : "";
        // RFC 038：Context 引擎为 Host 级共享组合根（跨会话复用，不每会话新建）；
        // 本会话持自身 AIContextSession 作会话态载体，provider 经其按名读写自身会话态。
        _contextEngine = contextEngine;
        _contextSession = new AIContextSession(_sessionId);
        _transcript = new List<AIMessage>();
        _requestMessages = new List<AIMessage>();
        _turn = AITurnState.Idle;
        _budget = new AISessionBudget();
        _budget.MaxTurns = options.MaxTurns;
        _budget.MaxMessages = options.MaxMessages;
        _budget.TurnsUsed = 0;
        _budget.MessagesUsed = 0;
        _humanGate = new AIHumanGate();
        _dispatcher = new AIToolDispatcher(sandbox);
        _pump = new AISessionStreamPump(sandbox, takeOverHandler, toolHandler);
        _disposed = false;
        _trimCount = 0;
        _lastToolSignature = "";
        _consecutiveIdenticalRounds = 0;
        _textDelta = null;
        _toolInvoked = null;
        _toolCompleted = null;
        _usageReported = null;
        _decisionEvents = new List<AIDecisionEvent>();
        _decisionEventReported = null;
        _totalPromptTokens = 0;
        _totalCompletionTokens = 0;
        _totalCacheReadTokens = 0;
        _totalCacheCreationTokens = 0;
        _lastUserText = "";
        _sawToolThisTurn = false;
    }

    public string SessionId { get { return _sessionId; } }
    public List<AIMessage> Transcript { get { return _transcript; } }
    public AITurnState Turn { get { return _turn; } }
    public AISessionOptions Options { get { return _options; } }
    /// <summary>预算上限（回合数；0 = 不设限）。</summary>
    internal int MaxTurns { get { return _budget.MaxTurns; } }
    /// <summary>预算上限（消息条数；0 = 不设限）。</summary>
    internal int MaxMessages { get { return _budget.MaxMessages; } }
    /// <summary>已用回合数。</summary>
    internal int TurnsUsed { get { return _budget.TurnsUsed; } }
    /// <summary>已用消息条数。</summary>
    internal int MessagesUsed { get { return _budget.MessagesUsed; } }
    /// <summary>剩余回合额度（0 = 已用尽；-1 = 不设限）。</summary>
    internal int RemainingTurns {
        get {
            if (_budget.MaxTurns <= 0) { return -1; }
            int rem = _budget.MaxTurns - _budget.TurnsUsed;
            return rem > 0 ? rem : 0;
        }
    }
    /// <summary>剩余消息额度（0 = 已用尽；-1 = 不设限）。</summary>
    internal int RemainingMessages {
        get {
            if (_budget.MaxMessages <= 0) { return -1; }
            int rem = _budget.MaxMessages - _budget.MessagesUsed;
            return rem > 0 ? rem : 0;
        }
    }
    public AIHumanRequest PendingHuman { get { return _humanGate.PendingHuman; } }
    internal AIToolStreamState ActiveToolStream { get { return _pump.ActiveToolStream; } }
    public AIWiki Wiki { get { return _wiki; } }
    public AIHumanOutcome LastHumanOutcome { get { return _humanGate.LastOutcome; } }
    public AIToolSet Tools { get { return _options != null ? _options.Tools : null; } }
    public AICapabilitySet Capabilities { get { return _options != null ? _options.Capabilities : null; } }
    public AIToolSandbox Sandbox { get { return _sandbox; } }
    /// <summary>窗口裁剪次数（RFC 038 可观测；0 = 从未裁剪）。</summary>
    public int TrimCount { get { return _trimCount; } }
    /// <summary>上下文工程组合根（RFC 038）：开发者经 Context.AddProvider(...) 注入自定义上下文源。</summary>
    public AIContextEngine Context { get { return _contextEngine; } }

    /// <summary>流式文本增量（业务端边生成边显示；非流式不触发，取 reply.Text）。</summary>
    public Action<string> TextDelta { get { return _textDelta; } set { _textDelta = value; } }
    /// <summary>工具即将执行（载荷为完整参数的工具调用；执行前触发）。</summary>
    public Action<AIToolCall> ToolInvoked { get { return _toolInvoked; } set { _toolInvoked = value; } }
    /// <summary>工具执行完成（载荷为调用 + 结果；执行后触发）。</summary>
    public Action<AIToolCall, AIToolResult> ToolCompleted { get { return _toolCompleted; } set { _toolCompleted = value; } }
    /// <summary>每轮 token 用量上报（业务端计费/监控/缓存命中观察）。</summary>
    public Action<AITokenUsage> UsageReported { get { return _usageReported; } set { _usageReported = value; } }
    /// <summary>会话决策轨迹（RFC 038 §13 / RFC 043 M5–M6：append-only 单一决策轨迹）。</summary>
    public List<AIDecisionEvent> DecisionEvents { get { return _decisionEvents; } }
    /// <summary>决策事件上报（业务端持久化/审计订阅）。</summary>
    public Action<AIDecisionEvent> DecisionEventReported { get { return _decisionEventReported; } set { _decisionEventReported = value; } }
    /// <summary>会话累计 token 用量（含缓存命中/写入；Clear 后归零）。</summary>
    public AITokenUsage TotalUsage {
        get {
            AITokenUsage u = new AITokenUsage();
            u.PromptTokens = _totalPromptTokens;
            u.CompletionTokens = _totalCompletionTokens;
            u.TotalTokens = _totalPromptTokens + _totalCompletionTokens;
            u.CacheReadTokens = _totalCacheReadTokens;
            u.CacheCreationTokens = _totalCacheCreationTokens;
            return u;
        }
    }

    public async Task<AIReply> RunAsync(string userText, CancellationToken cancellationToken) {
        this.ThrowIfDisposed();
        if (_turn != AITurnState.Idle && _turn != AITurnState.Done) {
            _turn = AITurnState.Failed;
            return AIReply.Fail("InvalidTurn", "AISession.RunAsync: turn not Idle/Done");
        }
        if (cancellationToken.IsCancellationRequested) {
            _turn = AITurnState.Cancelled;
            return AIReply.Fail("Cancelled", "AISession.RunAsync: canceled before start");
        }
        if (!_budget.CanStartTurn()) {
            _turn = AITurnState.Failed;
            return AIReply.Fail("MaxTurns", "AISession.RunAsync: MaxTurns exceeded");
        }
        if (!_budget.CanAddMessages(1)) {
            _turn = AITurnState.Failed;
            return AIReply.Fail("MaxMessages", "AISession.RunAsync: MaxMessages exceeded");
        }

        string text = userText != null ? userText : "";
        _lastUserText = text;
        // Instructions 由 AIContextEngine 单一组装（Rules 层最前；前缀稳定 → KV cache 命中）。
        this.AppendMessage(AIRole.User, text);
        _budget.TurnsUsed = _budget.TurnsUsed + 1;
        _pump.ResetPending();
        _lastToolSignature = "";
        _consecutiveIdenticalRounds = 0;
        _sawToolThisTurn = false;
        _turn = AITurnState.Completing;
        return await this.RunLoopAsync(cancellationToken);
    }

    /// <summary>单一回合循环（RFC 038）：Completing →（工具 → Completing）→ 文本或预算上限。</summary>
    private async Task<AIReply> RunLoopAsync(CancellationToken cancellationToken) {
        int rounds = 0;
        int maxRounds = _budget.MaxTurns > 0 ? _budget.MaxTurns : 16;
        // 工具回合后的中间文本候选最终答复（模型输出耗尽/预算上限时取最后一条文本）。
        string pendingText = "";
        // 无进展回合计数（推理模型仅回 reasoning、无 text 无 tool_calls）。连续超限即终结，
        // 且不消耗 MaxTurns——避免空转烧预算（每次空回复仍真实走一次 Provider 请求）。
        int emptyRounds = 0;
        while (true) {
            if (cancellationToken.IsCancellationRequested) {
                _turn = AITurnState.Cancelled;
                return AIReply.Fail("Cancelled", "AISession.RunLoop: canceled");
            }
            if (_turn == AITurnState.Cancelled) {
                return AIReply.Fail("Cancelled", "turn cancelled during tools");
            }
            _turn = AITurnState.Completing;

            AIReply modelReply = await this.NextModelReplyAsync(cancellationToken);
            if (cancellationToken.IsCancellationRequested) {
                _turn = AITurnState.Cancelled;
                return AIReply.Fail("Cancelled", "AISession.RunLoop: canceled after provider");
            }
            if (modelReply == null) {
                _turn = AITurnState.Failed;
                return AIReply.Fail("NullReply", "provider returned null");
            }
            if (modelReply.IsError) {
                // 工具回合后续回合若已收到中间文本，且模型输出耗尽（回放耗尽/无更多输出）→
                // 以最后一条文本作为最终答复，而非报错。
                if (_sawToolThisTurn && pendingText != "") {
                    _turn = AITurnState.Done;
                    _pump.ClearActiveStream();
                    return AIReply.FromText(pendingText);
                }
                _turn = AITurnState.Failed;
                return modelReply;
            }
            // token 用量上报（Provider 填充 reply.Usage；流式经 collector.OnUsage）。
            if (modelReply.Usage != null) {
                this.ReportUsage(modelReply.Usage);
            }

            // 收集本轮待执行工具调用：流式路径经 Pump 排队；非流式来自 reply.ToolCalls。
            List<AIToolCall> calls = new List<AIToolCall>();
            if (_options.IsStreaming) {
                calls = _pump.TakePendingCalls();
            } else if (modelReply.ToolCalls != null && modelReply.ToolCalls.Count > 0) {
                calls = modelReply.ToolCalls;
            }
            bool hadTools = _pump.ConsumeToolActivity() || (modelReply.ToolCalls != null && modelReply.ToolCalls.Count > 0);

            if (calls.Count == 0 && !hadTools) {
                // 纯文本回复。
                string assistantText = modelReply.Text != null ? modelReply.Text : "";
                if (assistantText == "") {
                    // 无进展回合：推理模型可能只回 reasoning_content、content 空。视为无效回合，
                    // 不消耗 MaxTurns；首空触发一次友好续问，仍空则优雅终结（不退化为「跳过所有后续」）。
                    emptyRounds = emptyRounds + 1;
                    if (emptyRounds >= 2) {
                        _turn = AITurnState.Done;
                        _pump.ClearActiveStream();
                        return AIReply.FromText(_sawToolThisTurn ? pendingText : "");
                    }
                    if (_budget.CanAddMessages(1)) {
                        this.AppendMessage(AIRole.System, "Please continue with your response.");
                    }
                    _turn = AITurnState.Completing;
                    continue;
                }
                emptyRounds = 0;
                if (!_budget.CanAddMessages(1)) {
                    _turn = AITurnState.Failed;
                    return AIReply.Fail("MaxMessages", "MaxMessages exceeded appending assistant");
                }
                AIMessage pureMsg = this.AppendMessage(AIRole.Assistant, assistantText, "", null, modelReply.ReasoningContent);
                await this.ProcessMessageAsync(pureMsg, cancellationToken);
                if (!_sawToolThisTurn) {
                    // 纯聊天（无工具）：文本即终结。
                    _turn = AITurnState.Done;
                    _pump.ClearActiveStream();
                    return AIReply.FromText(assistantText);
                }
                // 工具回合后的文本：可能是中间进度文本（模型尚未给出最终答复）。
                // 记为候选最终文本并续回合，直至模型输出耗尽（取最后一条文本）或预算上限。
                pendingText = assistantText;
                rounds = rounds + 1;
                if (rounds >= maxRounds) {
                    _turn = AITurnState.Failed;
                    return AIReply.Fail("MaxTurns", "AISession.RunLoop: text rounds exceeded MaxTurns");
                }
                _turn = AITurnState.Completing;
                continue;
            }

            // 有工具调用/工具活动 = 进展，重置无进展计数（单一空回复后若有工具进展，下次空回复重新计）。
            emptyRounds = 0;

            // 工具循环护栏预检（RFC 038）：同工具同签名连续超限 → 不执行本轮，直接失败（防烧钱）。
            // 签名 = 本轮全部工具调用名 + 参数 JSON 拼接（护栏 ≠ 规划：模型仍自由决策，宿主只防烧钱死循环）。
            string signature = this.ToolCallSignature(calls);
            if (signature != "") {
                if (signature == _lastToolSignature) {
                    _consecutiveIdenticalRounds = _consecutiveIdenticalRounds + 1;
                } else {
                    _lastToolSignature = signature;
                    _consecutiveIdenticalRounds = 1;
                }
                int limit = _options.MaxConsecutiveIdenticalToolRounds;
                if (limit > 0 && _consecutiveIdenticalRounds > limit) {
                    _turn = AITurnState.Failed;
                    if (_budget.CanAddMessages(1)) {
                        this.AppendMessage(AIRole.System, "ToolLoopGuard: identical tool rounds exceeded: " + signature);
                    }
                    return AIReply.Fail("ToolLoopGuard", "identical tool rounds exceeded MaxConsecutiveIdenticalToolRounds: " + signature);
                }
            }

            // 追加工具前的 assistant 消息（含 tool_calls 回显）。协议要求：每条 tool 结果
            // 消息前必须存在匹配的 assistant tool_calls 消息——故有调用时恒追加一条 assistant
            // 消息承载文本 + 回显；纯文本（无调用）才退化为仅非空文本追加。
            string preToolText = modelReply.Text != null ? modelReply.Text : "";
            List<AIToolCall> echoCalls = calls;
            if (echoCalls.Count == 0 && _pump.PendingTakeOverEcho.Count > 0) {
                // TakeOver 流式路径：调用已就地完成，无待派发调用，回显来自流式追踪。
                echoCalls = _pump.PendingTakeOverEcho;
            }
            if (echoCalls.Count > 0) {
                if (!_budget.CanAddMessages(1)) {
                    _turn = AITurnState.Failed;
                    return AIReply.Fail("MaxMessages", "MaxMessages exceeded appending assistant tool_calls");
                }
                AIMessage echoMsg = this.AppendMessage(AIRole.Assistant, preToolText, "", echoCalls, modelReply.ReasoningContent);
                await this.ProcessMessageAsync(echoMsg, cancellationToken);
            } else if (preToolText != "") {
                if (!_budget.CanAddMessages(1)) {
                    _turn = AITurnState.Failed;
                    return AIReply.Fail("MaxMessages", "MaxMessages exceeded appending assistant before tools");
                }
                AIMessage preMsg = this.AppendMessage(AIRole.Assistant, preToolText, "", null, modelReply.ReasoningContent);
                await this.ProcessMessageAsync(preMsg, cancellationToken);
            }

            // 延迟的 TakeOver 结果：assistant 回显之后按序写回（协议顺序）。
            List<AIToolResult> takeOverResults = _pump.PendingTakeOverResults;
            if (takeOverResults.Count > 0) {
                int n0 = takeOverResults.Count;
                int i0 = 0;
                while (i0 < n0) {
                    AIToolResult tr = takeOverResults[i0];
                    if (!_budget.CanAddMessages(1)) {
                        _turn = AITurnState.Failed;
                        return AIReply.Fail("MaxMessages", "MaxMessages exceeded appending take-over tool result");
                    }
                    AIMessage trMsg = this.AppendMessage(AIRole.Tool, tr.Content != null ? tr.Content : "", tr.CallId, null);
                    await this.ProcessMessageAsync(trMsg, cancellationToken);
                    i0 = i0 + 1;
                }
                _pump.ClearTakeOver();
            }

            // 逐个执行工具（异步；capability 拒绝不调用 handler；HITL 门闩可中断返回）
            _dispatcher.Begin(calls);
            AIReply gateReply = await this.DispatchAllAsync(cancellationToken);
            if (gateReply != null) {
                return gateReply;
            }
            _dispatcher.Reset();

            // 工具结果已写回 transcript → 续回合（Completing），直至文本或预算上限
            rounds = rounds + 1;
            if (rounds >= maxRounds) {
                _turn = AITurnState.Failed;
                return AIReply.Fail("MaxTurns", "AISession.RunLoop: tool rounds exceeded MaxTurns");
            }
            _turn = AITurnState.Completing;
        }
    }

    /// <summary>
    /// 逐批执行待派发工具：按顺序经 <see cref="AIToolSandbox"/> 执行（capability 拒绝不调用
    /// handler；HITL 门闩可中断返回）。遇 HITL 门闩返回 NeedsHuman（待 ResumeAsync 续）。
    /// 注：工具批当前为顺序 await（RFC 038 设计权衡待澄清——见 docs/plan.md）；结果按
    /// 调用序确定性写回 transcript。
    /// </summary>
    private async Task<AIReply> DispatchAllAsync(CancellationToken cancellationToken) {
        while (_dispatcher.HasPending()) {
            if (cancellationToken.IsCancellationRequested) {
                _turn = AITurnState.Cancelled;
                return AIReply.Fail("Cancelled", "canceled during tool dispatch");
            }
            AIToolCall call = _dispatcher.Current;
            AIReply gate = this.TryGateTool(call);
            if (gate != null) {
                return gate;
            }
            if (_sandbox == null) {
                _turn = AITurnState.Failed;
                return AIReply.Fail("NoSandbox", "ToolCalls present but no AIToolSet/sandbox bound");
            }
            if (_toolInvoked != null) {
                _toolInvoked(call);
            }
            AIToolResult result = await _dispatcher.ExecuteAndAdvanceAsync(cancellationToken);
            _sawToolThisTurn = true;
            if (_toolCompleted != null) {
                _toolCompleted(call, result);
            }
            if (!_budget.CanAddMessages(1)) {
                _turn = AITurnState.Failed;
                return AIReply.Fail("MaxMessages", "MaxMessages exceeded appending tool result");
            }
            AIMessage toolMsg = this.AppendMessage(AIRole.Tool, result.Content != null ? result.Content : "", call.CallId, null);
            await this.ProcessMessageAsync(toolMsg, cancellationToken);
            if (result.IsError) {
                _turn = AITurnState.Failed;
                return AIReply.Fail(result.ErrorKind != null ? result.ErrorKind : "ToolError", result.Content);
            }
        }
        return null;
    }

    /// <summary>HITL 门闩检查：RequireApproval / ApproveAllTools 需确认时进入 AwaitingHuman 并返回门闩回复。</summary>
    private AIReply TryGateTool(AIToolCall call) {
        bool needs = false;
        if (_options.ApproveAllTools) {
            needs = true;
        } else if (_sandbox != null && call != null) {
            AIToolDescriptor desc = _sandbox.Tools.FindDescriptor(call.Name);
            needs = desc != null && desc.RequireApproval;
        }
        if (!needs || call == null) {
            return null;
        }
        AIHumanRequest req = new AIHumanRequest("require-approval", "Human approval required for tool: " + call.Name);
        req.ToolCallId = call.CallId;
        req.ToolName = call.Name;
        req.ToolArguments = call.ArgumentsJson;
        _humanGate.EnterAwaiting(req);
        _turn = AITurnState.AwaitingHuman;
        AIReply gate = AIReply.FromText("");
        gate.NeedsHuman = true;
        gate.Gate = req;
        return gate;
    }

    /// <summary>调 Provider 取下一回复（流式消费 IAsyncEnumerable&lt;AIStreamEvent&gt; 事件流；
    /// 非流式 CompleteAsync）。Provider 异常收敛（AG-1）：不逃逸出 RunAsync，转 Fail 回复保证回合总能返回。</summary>
    private async Task<AIReply> NextModelReplyAsync(CancellationToken cancellationToken) {
        try {
            AIRequest request = await this.BuildRequestAsync(cancellationToken);
            if (_options.IsStreaming) {
                IAsyncEnumerable<AIStreamEvent> stream = _provider.StreamEventsAsync(request, cancellationToken);
                AISessionStreamCollector collector = new AISessionStreamCollector(this);
                return await collector.CollectAsync(stream, cancellationToken);
            }
            Task<AIReply> completeTask = _provider.CompleteAsync(request, cancellationToken);
            return await completeTask;
        } catch (Exception ex) {
            string msg = ex != null && ex.Message != null ? ex.Message : "provider error";
            return AIReply.Fail("ProviderError", "provider threw: " + msg);
        }
    }

    public void Clear() {
        this.ThrowIfDisposed();
        _transcript = new List<AIMessage>();
        _requestMessages = new List<AIMessage>();
        _pump.ResetPending();
        _dispatcher.Reset();
        _budget.TurnsUsed = 0;
        _budget.MessagesUsed = 0;
        _turn = AITurnState.Idle;
        _humanGate.Reset();
        _trimCount = 0;
        _lastToolSignature = "";
        _consecutiveIdenticalRounds = 0;
        _sawToolThisTurn = false;
        _textDelta = null;
        _toolInvoked = null;
        _toolCompleted = null;
        _usageReported = null;
        _decisionEventReported = null;
        _totalPromptTokens = 0;
        _totalCompletionTokens = 0;
        _totalCacheReadTokens = 0;
        _totalCacheCreationTokens = 0;
        _lastUserText = "";
    }

    public AISessionSnapshot Snapshot() {
        this.ThrowIfDisposed();
        AISessionSnapshot snap = new AISessionSnapshot();
        snap.SessionId = _sessionId;
        snap.Turn = _turn;
        snap.Budget = new AISessionBudget();
        snap.Budget.MaxTurns = _budget.MaxTurns;
        snap.Budget.MaxMessages = _budget.MaxMessages;
        snap.Budget.TurnsUsed = _budget.TurnsUsed;
        snap.Budget.MessagesUsed = _budget.MessagesUsed;
        snap.Transcript = new List<AIMessage>();
        int i = 0; int n = _transcript.Count;
        while (i < n) {
            AIMessage m = _transcript[i];
            AIMessage copy = new AIMessage(m.Role, m.Content, m.ToolCallId, m.ToolCalls);
            copy.ReasoningContent = m.ReasoningContent;
            snap.Transcript.Add(copy);
            i = i + 1;
        }
        return snap;
    }

    public void Restore(AISessionSnapshot snapshot) {
        this.ThrowIfDisposed();
        if (snapshot == null) { throw new ArgumentNullException("snapshot"); }
        _sessionId = snapshot.SessionId != null ? snapshot.SessionId : "";
        _turn = snapshot.Turn;
        _budget = new AISessionBudget();
        // 先以全新预算重建 transcript（AppendMessage 会累加 MessagesUsed），
        // 再以快照值覆盖预算——避免 Restore 后 MessagesUsed 被双倍计数（P4）。
        _transcript = new List<AIMessage>();
        _requestMessages = new List<AIMessage>();
        if (snapshot.Transcript != null) {
            int i = 0; int n = snapshot.Transcript.Count;
            while (i < n) {
                AIMessage m = snapshot.Transcript[i];
                this.AppendMessage(m.Role, m.Content, m.ToolCallId, m.ToolCalls, m.ReasoningContent);
                i = i + 1;
            }
        }
        if (snapshot.Budget != null) {
            _budget.MaxTurns = snapshot.Budget.MaxTurns;
            _budget.MaxMessages = snapshot.Budget.MaxMessages;
            _budget.TurnsUsed = snapshot.Budget.TurnsUsed;
            _budget.MessagesUsed = snapshot.Budget.MessagesUsed;
        }
        _humanGate.Reset();
        _pump.ResetPending();
        _dispatcher.Reset();
        _trimCount = 0;
        _lastToolSignature = "";
        _consecutiveIdenticalRounds = 0;
        _sawToolThisTurn = false;
        _textDelta = null;
        _toolInvoked = null;
        _toolCompleted = null;
        _usageReported = null;
        _decisionEventReported = null;
        _totalPromptTokens = 0;
        _totalCompletionTokens = 0;
        _totalCacheReadTokens = 0;
        _totalCacheCreationTokens = 0;
        _lastUserText = "";
        // Instructions 由 AIContextEngine 每次 BuildAsync 重新组装（单一事实源 = options.Instructions），
        // 不随 transcript 快照固化；Restore 后下次请求仍经引擎注入最前。
    }

    public AIToolStreamDisposition PumpToolCallStart(AIToolCallStart start, CancellationToken cancellationToken) {
        this.ThrowIfDisposed();
        if (cancellationToken.IsCancellationRequested) {
            _turn = AITurnState.Cancelled; _pump.ClearActiveStream();
            return AIToolStreamDisposition.Reject;
        }
        if (_turn != AITurnState.Completing && _turn != AITurnState.StreamingTools) {
            throw new InvalidOperationException("PumpToolCallStart: expected Completing/StreamingTools");
        }
        _turn = AITurnState.StreamingTools;
        AIToolStreamDisposition d = _pump.OnToolCallStart(start, cancellationToken);
        if (d == AIToolStreamDisposition.Reject) {
            _turn = AITurnState.Failed; _pump.ClearActiveStream();
        }
        return d;
    }

    public void PumpToolArgDelta(AIToolArgDelta delta, CancellationToken cancellationToken) {
        this.ThrowIfDisposed();
        if (cancellationToken.IsCancellationRequested) {
            _turn = AITurnState.Cancelled; _pump.ClearActiveStream(); return;
        }
        if (_turn != AITurnState.StreamingTools) {
            throw new InvalidOperationException("PumpToolArgDelta: expected StreamingTools");
        }
        _pump.OnToolArgDelta(delta, cancellationToken);
    }

    public AIToolResult PumpToolCallEnd(AIToolCallEnd end, CancellationToken cancellationToken) {
        this.ThrowIfDisposed();
        if (cancellationToken.IsCancellationRequested) {
            _turn = AITurnState.Cancelled; _pump.ClearActiveStream();
            return new AIToolResult(end != null ? end.CallId : "", "", true);
        }
        if (_turn != AITurnState.StreamingTools) {
            throw new InvalidOperationException("PumpToolCallEnd: expected StreamingTools");
        }
        AIToolResult result = _pump.OnToolCallEnd(end, cancellationToken);
        _turn = AITurnState.AwaitingTools;
        if (result.IsError && result.ErrorKind == "CapabilityDenied") {
            _turn = AITurnState.Failed;
            return result;
        }
        return result;
    }

    public void EnterAwaitingHuman(AIHumanRequest request) {
        this.ThrowIfDisposed();
        if (request == null) { throw new ArgumentNullException("request"); }
        _humanGate.EnterAwaiting(request);
        _turn = AITurnState.AwaitingHuman;
    }

    public Task ApproveAsync(AIToolCall edited, CancellationToken cancellationToken) {
        this.ThrowIfDisposed();
        if (_turn != AITurnState.AwaitingHuman) {
            throw new InvalidOperationException("ApproveAsync: turn is not AwaitingHuman");
        }
        string name = "";
        string args = "";
        if (edited != null) {
            name = edited.Name != null ? edited.Name : "";
            args = edited.ArgumentsJson != null ? edited.ArgumentsJson : "";
        }
        Task t = _humanGate.ApproveAsync(name, args, cancellationToken);
        _turn = AITurnState.AwaitingTools;
        return t;
    }

    public Task RejectAsync(string reason, CancellationToken cancellationToken) {
        this.ThrowIfDisposed();
        if (_turn != AITurnState.AwaitingHuman) {
            throw new InvalidOperationException("RejectAsync: turn is not AwaitingHuman");
        }
        Task t = _humanGate.RejectAsync(reason, cancellationToken);
        string msg = reason != null ? reason : "";
        if (_budget.CanAddMessages(1)) {
            this.AppendMessage(AIRole.System, "HITL rejected: " + msg);
        }
        _turn = AITurnState.Failed;
        return t;
    }

    public Task ProvideInputAsync(string text, CancellationToken cancellationToken) {
        this.ThrowIfDisposed();
        if (_turn != AITurnState.AwaitingHuman) {
            throw new InvalidOperationException("ProvideInputAsync: turn is not AwaitingHuman");
        }
        Task t = _humanGate.ProvideInputAsync(text, cancellationToken);
        string body = text != null ? text : "";
        if (_budget.CanAddMessages(1)) {
            this.AppendMessage(AIRole.User, body);
        }
        _turn = AITurnState.Completing;
        return t;
    }

    /// <summary>
    /// HITL 门闩续回合（RFC 038「RunAsync 返回到门闩、由调用方 Resume」唯一正道）：
    /// Approve → 执行编辑后 call → 续派发与回合；Reject → 写回 transcript + 回合 Failed；Input → 回 Completing。
    /// </summary>
    public async Task<AIReply> ResumeAsync(CancellationToken cancellationToken) {
        this.ThrowIfDisposed();
        AIHumanOutcome outcome = _humanGate.LastOutcome;
        if (outcome == null) {
            throw new InvalidOperationException("ResumeAsync: gate not closed; call ApproveAsync/RejectAsync/ProvideInputAsync first");
        }
        if (outcome.Decision == AIHumanDecision.Cancelled) {
            _turn = AITurnState.Cancelled;
            _dispatcher.Reset();
            return AIReply.Fail("Cancelled", "turn cancelled by human");
        }
        if (outcome.Decision == AIHumanDecision.Rejected) {
            // transcript 的 "HITL rejected" System 消息已在 RejectAsync 写入（用户动作入口），
            // 此处只做状态转换，避免双写（模型收到两条重复系统消息）。
            _turn = AITurnState.Failed;
            _dispatcher.Reset();
            return AIReply.Fail("Rejected", outcome.RejectReason);
        }
        if (outcome.Decision == AIHumanDecision.InputProvided) {
            // 人类补充输入已由 ProvideInputAsync 写入 User 消息，此处只续回合。
            _dispatcher.Reset();
            _turn = AITurnState.Completing;
            return await this.RunLoopAsync(cancellationToken);
        }
        // Approved → 执行编辑后的工具调用，继续派发与回合
        _turn = AITurnState.DispatchingTools;
        string pendingCallId = _dispatcher.CurrentCallId();
        AIToolCall edited = new AIToolCall(pendingCallId, outcome.EditedToolName, outcome.EditedToolArguments);
        if (_sandbox == null) {
            _turn = AITurnState.Failed;
            _dispatcher.Reset();
            return AIReply.Fail("NoSandbox", "ToolCalls present but no AIToolSet/sandbox bound");
        }
        if (_toolInvoked != null) {
            _toolInvoked(edited);
        }
        AIToolResult result = await _sandbox.ExecuteAsync(edited, cancellationToken);
        _sawToolThisTurn = true;
        if (_toolCompleted != null) {
            _toolCompleted(edited, result);
        }
        if (cancellationToken.IsCancellationRequested) {
            _turn = AITurnState.Cancelled;
            return AIReply.Fail("Cancelled", "canceled after approval");
        }
        if (!_budget.CanAddMessages(1)) {
            _turn = AITurnState.Failed;
            return AIReply.Fail("MaxMessages", "MaxMessages exceeded appending approved tool result");
        }
        AIMessage appMsg = this.AppendMessage(AIRole.Tool, result.Content != null ? result.Content : "", pendingCallId, null);
        await this.ProcessMessageAsync(appMsg, cancellationToken);
        if (result.IsError) {
            _turn = AITurnState.Failed;
            return AIReply.Fail(result.ErrorKind != null ? result.ErrorKind : "ToolError", result.Content);
        }
        _dispatcher.SkipCurrent();
        AIReply gate2 = await this.DispatchAllAsync(cancellationToken);
        if (gate2 != null) {
            return gate2;
        }
        _dispatcher.Reset();
        _turn = AITurnState.Completing;
        return await this.RunLoopAsync(cancellationToken);
    }

    public void Dispose() {
        if (_disposed) { return; }
        _disposed = true;
        _humanGate.Reset();
        _pump.ClearActiveStream();
        _dispatcher.Reset();
    }

    /// <summary>追加消息到 transcript + 请求消息（增量 append-only，避免每轮全量拷贝 O(n²)）。</summary>
    private AIMessage AppendMessage(AIRole role, string content) {
        // 编译器重载解析已修复（B2：同名非 async 重载不再误解析回自身）→ 直接委托 5 参重载。
        return this.AppendMessage(role, content, "", null, "");
    }

    /// <summary>
    /// 追加消息（含 OpenAI 兼容工具关联面）。assistant 消息传 <paramref name="toolCalls"/>（回显），
    /// tool 结果消息传 <paramref name="toolCallId"/>；其余角色均空。
    /// </summary>
    private AIMessage AppendMessage(AIRole role, string content, string toolCallId, List<AIToolCall> toolCalls) {
        return this.AppendMessage(role, content, toolCallId, toolCalls, "");
    }

    /// <summary>
    /// 追加消息（含思维链关联面）。<paramref name="reasoningContent"/> 为 assistant 思考链内容
    /// （DeepSeek reasoning_content）；思考模式下工具调用轮次须随 assistant 消息保存并在后续请求回传。
    /// 返回新追加的 <see cref="AIMessage"/>（供调用后方向 ProcessMessageAsync 抽取 / 持久化）。
    /// </summary>
    private AIMessage AppendMessage(AIRole role, string content, string toolCallId, List<AIToolCall> toolCalls, string reasoningContent) {
        AIMessage m = new AIMessage(role, content, toolCallId, toolCalls);
        m.ReasoningContent = reasoningContent != null ? reasoningContent : "";
        _transcript.Add(m);
        _requestMessages.Add(m);
        _budget.MessagesUsed = _budget.MessagesUsed + 1;
        return m;
    }

    /// <summary>
    /// RFC 038 调用后方向：把刚追加的消息投递给 Host 级共享引擎的全部 provider
    /// （经本会话 AIContextSession）。容错：单源异常已由引擎内部跳过，此处再兜底一层，
    /// 失败不打断回合主流程。
    /// </summary>
    private async Task ProcessMessageAsync(AIMessage m, CancellationToken ct) {
        if (m == null || _contextEngine == null) {
            return;
        }
        try {
            await _contextEngine.ProcessMessageAsync(m, _contextSession, ct);
        } catch {
            // 容错：调用后处理失败不影响回合主流程。
        }
    }

    private async Task<AIRequest> BuildRequestAsync(CancellationToken cancellationToken) {
        AIRequest request = new AIRequest();
        // RFC 038：窗口裁剪单一锁定点（BuildRequest 前；禁旁路）。
        // 系统消息若有 + 最近 K 条；transcript 保持完整（裁剪仅影响请求面，可审计）。
        int keep = _options.WindowKeepLast;
        int nBefore = _requestMessages.Count;
        _requestMessages = AIWindowManager.Trim(_requestMessages, keep);
        if (keep > 0 && nBefore > keep) {
            _trimCount = _trimCount + 1;
        }
        // 上下文工程组合根一次异步组装（查询感知 + 预算裁剪 + 生命周期编排）：消息（全 provider
        // 合并）+ 聚合工具（主工具 + 激活 Skill 工具）。查询感知：向动态源注入当前用户请求 /
        // 回合序号，支撑 RAG 等真实场景。
        AIContextQuery ctxQuery = new AIContextQuery(_sessionId, _lastUserText, _budget.TurnsUsed);
        AIContextSource ctx = null;
        if (_contextEngine != null) {
            // RFC 038：Host 级共享引擎；传本会话的 AIContextSession 承载会话态。
            ctx = await _contextEngine.BuildAsync(ctxQuery, _contextSession, cancellationToken);
        } else {
            ctx = new AIContextSource();
        }
        List<AIMessage> attached = ctx.Messages;
        // 附加上下文稳定顺序 → 前缀缓存友好。有附加时请求面 = 附加上下文 + 会话消息（不污染 append-only 面）。
        if (attached.Count > 0) {
            List<AIMessage> merged = new List<AIMessage>();
            int k = 0;
            while (k < attached.Count) {
                merged.Add(attached[k]);
                k = k + 1;
            }
            int m = 0;
            while (m < _requestMessages.Count) {
                merged.Add(_requestMessages[m]);
                m = m + 1;
            }
            request.Messages = merged;
        } else {
            request.Messages = _requestMessages;
        }
        // 工具聚合（会话有效工具集 = 主工具 + 激活 Skill 工具）：CreateSession 已把
        // 有效工具解析进 _options.Tools（显式 > 宿主 > 默认工具源 [AITool]），此处以
        // 会话选项为唯一事实源合成请求面 schema——默认工具源（编译期 [AITool]）因此
        // 真实进入模型请求面。ctx.Tools 仅承载 Host 级主工具（_mainTools=host.Options.Tools），
        // 默认工具源在 CreateSession 晚于引擎构造解析，会漏掉 → 模型幻觉非 Arc 格式工具调用。
        AIToolSet requestTools = this.AggregateRequestTools();
        request.ToolsJson = requestTools.BuildSchemasJson();
        // 响应格式契约透传（MAF contract-first；Provider 按协议内部映射）。
        request.ResponseFormat = _options != null ? _options.ResponseFormat : null;
        return request;
    }

    /// <summary>
    /// 会话有效工具集聚合（请求面事实源）：主工具（_options.Tools，已含默认工具源解析）
    /// + 激活 Skill 工具。与 sandbox 执行面（主工具 + AttachExternalTools(skill)）一致，
    /// 确保模型看到的工具 = 会话实际可执行的工具。
    /// </summary>
    private AIToolSet AggregateRequestTools() {
        AIToolSet merged = new AIToolSet();
        AIToolSet main = _options != null ? _options.Tools : null;
        if (main != null) {
            main.ForEach((d: AIToolDescriptor, h: AIToolHandler) => { merged.Add(d, h); });
        }
        if (_options != null && _options.Skills != null) {
            AIToolSet skillTools = _options.Skills.ToToolSet();
            skillTools.ForEach((d: AIToolDescriptor, h: AIToolHandler) => { merged.Add(d, h); });
        }
        return merged;
    }

    /// <summary>RFC 038：本轮工具调用签名（名称 + 参数 JSON 拼接；空 = 无工具）。</summary>
    private string ToolCallSignature(List<AIToolCall> calls) {
        if (calls == null || calls.Count == 0) {
            return "";
        }
        string sig = "";
        int i = 0;
        int n = calls.Count;
        while (i < n) {
            AIToolCall c = calls[i];
            string name = c != null && c.Name != null ? c.Name : "";
            string args = c != null && c.ArgumentsJson != null ? c.ArgumentsJson : "";
            sig = sig + name + "|" + args + ";";
            i = i + 1;
        }
        return sig;
    }

    private void ThrowIfDisposed() {
        if (_disposed) { throw new ObjectDisposedException("AISession"); }
    }

    /// <summary>流式文本增量内部通路（AISessionStreamCollector 投递 → 业务端 TextDelta）。</summary>
    internal void NotifyTextDelta(string text) {
        if (text == null || text == "") { return; }
        if (_textDelta != null) { _textDelta(text); }
    }

    /// <summary>token 用量汇总 + UsageReported 事件（非流式 reply.Usage 与流式 collector.OnUsage 统一入口）。</summary>
    internal void ReportUsage(AITokenUsage usage) {
        if (usage == null) { return; }
        _totalPromptTokens = _totalPromptTokens + usage.PromptTokens;
        _totalCompletionTokens = _totalCompletionTokens + usage.CompletionTokens;
        _totalCacheReadTokens = _totalCacheReadTokens + usage.CacheReadTokens;
        _totalCacheCreationTokens = _totalCacheCreationTokens + usage.CacheCreationTokens;
        if (_usageReported != null) {
            _usageReported(usage);
        }
    }

    /// <summary>追加一条决策事件（append-only；kind 与 038 approval 并列，见 AIDecisionEvent）。</summary>
    internal void AppendDecisionEvent(AIDecisionEventKind kind, string detail, string reason, int revision) {
        AIDecisionEvent e = AIDecisionEvent.Create(kind, detail, reason, revision);
        _decisionEvents.Add(e);
        if (_decisionEventReported != null) {
            _decisionEventReported(e);
        }
    }
}
