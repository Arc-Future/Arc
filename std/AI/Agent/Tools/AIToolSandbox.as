// RFC 038: sandbox — capability gate + AIToolSet invoke + stream disposition.
namespace Arc.Agent;

using Arc;
using Arc.Collections;

/// <summary>
/// Host-side tool sandbox. CapabilityDenied never invokes handlers.
/// Buffer path uses concrete AIBufferingStreamHandler; TakeOver uses concrete field
/// (abstract virtual mid-stream from Session.Pump* is unreliable — language gap).
/// </summary>
public class AIToolSandbox : AIToolStreamHandler {
    private AIToolSet _externalTools;
    private AIBufferingStreamHandler _bufferInner;
    private AITakeOverStreamHandler _takeOverInner;
    private AIToolStreamDisposition _disposition;
    private string _callId;
    private string _toolName;
    private bool _capDenied;

    public AIToolSandbox(AIToolSet tools, AICapabilitySet capabilities) {
        Tools = tools != null ? tools : new AIToolSet();
        _externalTools = new AIToolSet();
        Capabilities = capabilities != null ? capabilities : new AICapabilitySet();
        _bufferInner = new AIBufferingStreamHandler();
        _takeOverInner = null;
        _disposition = AIToolStreamDisposition.Buffer;
        _callId = "";
        _toolName = "";
        _capDenied = false;
        Results = new List<AIToolResult>();
        HandlerInvokeCount = 0;
        PlanGate = null;
        LeaseGate = null;
    }

    public AIToolSandbox(AIToolSet tools, AICapabilitySet capabilities, AITakeOverStreamHandler takeOver) {
        Tools = tools != null ? tools : new AIToolSet();
        _externalTools = new AIToolSet();
        Capabilities = capabilities != null ? capabilities : new AICapabilitySet();
        _bufferInner = new AIBufferingStreamHandler();
        _takeOverInner = takeOver;
        _disposition = AIToolStreamDisposition.Buffer;
        _callId = "";
        _toolName = "";
        _capDenied = false;
        Results = new List<AIToolResult>();
        HandlerInvokeCount = 0;
        PlanGate = null;
        LeaseGate = null;
    }

    /// <summary>计划门闩（AIHost 装配后注入；null = 未启用）。</summary>
    public AIPlanGate PlanGate { get; set; }

    /// <summary>
    /// 惰性 ToolPath 租约门（子代理协调器装配后注入；null = 未启用）。调度层在
    /// capability + 计划门闩通过后、handler 落盘前经它取租约：首次真实写命中声明写面
    /// → Acquire；被其它会话持有 → 拒绝（后到拒绝，工作项由宿主标 Failed）。
    /// 只读能力 / 非声明写面一律放行（读取不阻塞写入）。
    /// </summary>
    public AISubAgentLeaseGate LeaseGate { get; set; }

    public List<AIToolResult> Results { get; } = new List<AIToolResult>();

    public int HandlerInvokeCount { get; set; }

    public AIToolSet Tools { get; }

    /// <summary>
    /// 附加外部能力工具（如挂载 Skill 的工具）。执行/流式描述符查找时先主工具集、
    /// 再外部工具集（合并查找），使 Skill 工具可被模型调用。禁原地改写主工具集。
    /// </summary>
    public void AttachExternalTools(AIToolSet tools) {
        if (tools != null) {
            _externalTools = tools;
        }
    }

    public AICapabilitySet Capabilities { get; }

    public async Task<AIToolResult> ExecuteAsync(AIToolCall call, CancellationToken cancellationToken) {
        if (call == null) {
            return AIToolResult.Fail("", "InvalidCall", "null tool call");
        }
        string name = call.Name != null ? call.Name : "";
        string cid = call.CallId != null ? call.CallId : "";
        AIToolDescriptor desc = this.LookupDescriptor(name);
        // 能力门禁：以工具声明的 Capability 为准（缺省 ai.Tool）；未授权 → 拒绝且不调用 handler。
        string cap = desc != null && desc.Capability != null ? desc.Capability : "ai.Tool";
        if (!Capabilities.Contains(cap)) {
            AIToolResult denied = AIToolResult.CapabilityDenied(cid, name);
            Results.Add(denied);
            return denied;
        }
        // 调度层计划门闩：能力受计划门闩约束且存在未批准计划 → 拦截（错误对模型可见，
        // 提示等待审批/修订）。只读能力 / 无计划一律放行（简单任务不拦）。
        if (PlanGate != null && PlanGate.Blocks(cap)) {
            AIToolResult blocked = AIToolResult.Fail(cid, "PlanGatePending",
                "write blocked by plan gate: plan is PENDING APPROVAL — wait for the human to approve the plan before writing");
            Results.Add(blocked);
            return blocked;
        }
        // 调度层惰性 ToolPath 租约门（A1）：首次真实写前取租约；被其它会话持有 →
        // 拒绝（后到拒绝，可审计）。非写能力 / 非声明写面一律放行。
        if (LeaseGate != null && !LeaseGate.GuardWrite(cap, call.ArgumentsJson)) {
            AIToolResult conflict = AIToolResult.Fail(cid, "ToolPathLeaseConflict",
                "ToolPath write conflict: first real write denied (path held by another session) — work item will be marked Failed");
            Results.Add(conflict);
            return conflict;
        }
        AIToolHandler handler = this.LookupHandler(name);
        if (handler == null) {
            AIToolResult missing = AIToolResult.Fail(cid, "ToolNotFound", "tool not registered: " + name);
            Results.Add(missing);
            return missing;
        }
        if (cancellationToken.IsCancellationRequested) {
            AIToolResult cancelled = AIToolResult.Fail(cid, "Cancelled", "tool cancelled: " + name);
            Results.Add(cancelled);
            return cancelled;
        }
        HandlerInvokeCount = HandlerInvokeCount + 1;
        AIToolResult result = await AIToolSandbox.InvokeHandlerSafeAsync(handler, call, cid, name, cancellationToken);
        Results.Add(result);
        return result;
    }

    /// <summary>
    /// 工具异常收敛（AG-1）：捕获 handler 抛出的异常（含同步 throw 与异步 faulted），
    /// 转 <see cref="AIToolResult.Fail"/> 而非逃逸出会话状态机；null 结果同样归一为 Fail。
    /// </summary>
    private static async Task<AIToolResult> InvokeHandlerSafeAsync(
        AIToolHandler handler,
        AIToolCall call,
        string cid,
        string name,
        CancellationToken cancellationToken) {
        try {
            AIToolResult r = await handler.InvokeAsync(call, cancellationToken);
            if (r == null) {
                return AIToolResult.Fail(cid, "NullResult", "handler returned null");
            }
            return r;
        } catch (Exception ex) {
            string msg = ex != null && ex.Message != null ? ex.Message : "unknown error";
            return AIToolResult.Fail(cid, "ToolError", "tool '" + name + "' threw: " + msg);
        }
    }

    public override AIToolStreamDisposition OnToolCallStart(AIToolCallStart start, CancellationToken cancellationToken) {
        _callId = start != null && start.CallId != null ? start.CallId : "";
        _toolName = start != null && start.ToolName != null ? start.ToolName : "";
        _capDenied = false;
        AIToolDescriptor desc = this.LookupDescriptor(_toolName);
        string cap = desc != null ? desc.Capability : "ai.Tool";
        if (!Capabilities.Contains(cap)) {
            _capDenied = true;
            _disposition = AIToolStreamDisposition.Reject;
            return AIToolStreamDisposition.Reject;
        }
        // 调度层计划门闩（流式路径同步判定）：受约束且未批准计划的写入能力 → 拒绝。
        if (PlanGate != null && PlanGate.Blocks(cap)) {
            _capDenied = true;
            _disposition = AIToolStreamDisposition.Reject;
            return AIToolStreamDisposition.Reject;
        }
        if (_takeOverInner != null) {
            _disposition = _takeOverInner.OnToolCallStart(start, cancellationToken);
        } else {
            _disposition = _bufferInner.OnToolCallStart(start, cancellationToken);
        }
        return _disposition;
    }

    public override void OnToolArgDelta(AIToolArgDelta delta, CancellationToken cancellationToken) {
        if (_capDenied || _disposition == AIToolStreamDisposition.Reject) {
            return;
        }
        if (_disposition == AIToolStreamDisposition.TakeOver && _takeOverInner != null) {
            _takeOverInner.OnToolArgDelta(delta, cancellationToken);
        } else {
            _bufferInner.OnToolArgDelta(delta, cancellationToken);
        }
    }

    public override AIToolResult OnToolCallEnd(AIToolCallEnd end, CancellationToken cancellationToken) {
        string cid = end != null && end.CallId != null ? end.CallId : _callId;
        if (_capDenied || _disposition == AIToolStreamDisposition.Reject) {
            AIToolResult denied = AIToolResult.CapabilityDenied(cid, _toolName);
            Results.Add(denied);
            return denied;
        }
        if (_disposition == AIToolStreamDisposition.TakeOver && _takeOverInner != null) {
            AIToolResult taken = _takeOverInner.OnToolCallEnd(end, cancellationToken);
            if (taken == null) {
                taken = AIToolResult.Fail(cid, "TakeOverNull", "TakeOver handler returned null");
            }
            Results.Add(taken);
            return taken;
        }
        // Buffer 路径：仅收集完整 args（标记 BufferedArgs），不在此同步执行——
        // 工具执行统一由会话异步循环 await ExecuteAsync（§3.1.1；杜绝 SSE 循环内同步执行）。
        return _bufferInner.OnToolCallEnd(end, cancellationToken);
    }

    // ── 私有：合并工具查找（主工具集 → 外部工具集） ──

    private AIToolDescriptor LookupDescriptor(string name) {
        AIToolDescriptor d = Tools.FindDescriptor(name);
        if (d == null && _externalTools != null) {
            d = _externalTools.FindDescriptor(name);
        }
        return d;
    }

    private AIToolHandler LookupHandler(string name) {
        AIToolHandler h = Tools.FindHandler(name);
        if (h == null && _externalTools != null) {
            h = _externalTools.FindHandler(name);
        }
        return h;
    }
}
