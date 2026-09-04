// M8 CodeAct（RFC 038 §3.4.2）：模型生成代码执行门面。
//
// 安全底线：无论何种后端，均经 capability 白名单 fail-closed 授权——默认空白名单全拒，
// 仅显式授权 `codeact.CodeAct` 能力后放行；配额（超时/输出上限）由此门面统一持有并下发后端。
namespace Arc.Agent;
using Arc;

/// <summary>
/// CodeAct 门面。持可插拔 <see cref="IAICodeActProvider"/> 后端 + capability 门禁 + 配额。
/// 以工具形态经 <see cref="AIToolSandbox"/> 调度的同时，亦可作为一等能力直接调用。
/// </summary>
public class AICodeAct {
    /// <summary>执行任意代码所需的 capability 白名单键（fail-closed 门禁）。</summary>
    public const string CodeActCapability = "codeact.CodeAct";

    private AICapabilitySet _capabilities;
    private long _timeoutMs;
    private int _maxOutputChars;

    /// <summary>空白名单创建：fail-closed——未显式授权 CodeActCapability 一律拒绝。</summary>
    public AICodeAct(IAICodeActProvider provider) {
        this.Provider = provider;
        _capabilities = new AICapabilitySet();
        _timeoutMs = 30000;
        _maxOutputChars = 8000;
    }

    /// <summary>以显式 capability 白名单创建（须含 <see cref="CodeActCapability"/> 才放行）。</summary>
    public AICodeAct(IAICodeActProvider provider, AICapabilitySet capabilities) {
        this.Provider = provider;
        _capabilities = capabilities != null ? capabilities : new AICapabilitySet();
        _timeoutMs = 30000;
        _maxOutputChars = 8000;
    }

    public IAICodeActProvider Provider { get; }

    /// <summary>执行超时（毫秒；默认 30000）。</summary>
    public long TimeoutMs {
        get { return _timeoutMs; }
        set { _timeoutMs = value > 0 ? value : _timeoutMs; }
    }

    /// <summary>标准输出/错误各自输出上限字符数（默认 8000；超出截断）。</summary>
    public int MaxOutputChars {
        get { return _maxOutputChars; }
        set { _maxOutputChars = value > 0 ? value : _maxOutputChars; }
    }

    public async Task<AICodeActResult> ExecuteAsync(string code, CancellationToken cancellationToken) {
        return await this.ExecuteAsync(code, null, cancellationToken);
    }

    public async Task<AICodeActResult> ExecuteAsync(
        string code,
        Dictionary<string, string?> env,
        CancellationToken cancellationToken) {
        // fail-closed：白名单须含 CodeActCapability 才放行；未授权拒绝，无副作用。
        if (!_capabilities.Contains(CodeActCapability)) {
            return AICodeActResult.CapabilityDenied(
                "codeact execution not authorized (fail-closed; grant " + CodeActCapability + ")");
        }
        if (this.Provider == null) {
            return AICodeActResult.Fail("no codeact provider configured");
        }
        return await this.Provider.ExecuteAsync(code, env, _timeoutMs, _maxOutputChars, cancellationToken);
    }
}

/// <summary>
/// 内置工具适配器（internal）：把 <see cref="AICodeAct"/> 包装为 <see cref="AIToolHandler"/>，
/// 使模型生成的代码执行经 <see cref="AIToolSandbox"/> 统一走 capability 分派 + HITL 门闩
/// （RFC 038 §7），由 <see cref="AIHost.CreateCodeAct"/> 装配，开发者无需手工触发。
/// </summary>
internal class CodeActToolHandler : AIToolHandler {
    private AICodeAct _codeAct;

    public CodeActToolHandler(AICodeAct codeAct) {
        _codeAct = codeAct;
    }

    public override string Name {
        get { return "codeact"; }
    }

    public override string Capability {
        get { return AICodeAct.CodeActCapability; }
    }

    public override async Task<AIToolResult> InvokeAsync(AIToolCall call, CancellationToken cancellationToken) {
        string cid = call != null && call.CallId != null ? call.CallId : "";
        string args = call != null && call.ArgumentsJson != null ? call.ArgumentsJson : "";
        AIToolArgsReader reader = new AIToolArgsReader(args);
        string code = reader.GetString("code");
        if (code == "") {
            return AIToolResult.Fail(cid, "InvalidArgs", "codeact: missing 'code' argument");
        }
        AICodeActResult r = await _codeAct.ExecuteAsync(code, cancellationToken);
        if (r.Success) {
            return AIToolResult.Ok(cid, r.StandardOutput != null ? r.StandardOutput : "");
        }
        string kind = r.Cancelled ? "Cancelled" : (r.TimedOut ? "Timeout" : "CodeActError");
        string message = r.Error != null && r.Error != "" ? r.Error : (r.StandardError != null ? r.StandardError : "execution failed");
        return AIToolResult.Fail(cid, kind, message);
    }
}
