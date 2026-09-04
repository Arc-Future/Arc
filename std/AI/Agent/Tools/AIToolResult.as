// RFC 038: tool result — ErrorKind locked for capability deny (M1).
namespace Arc.Agent;

/// <summary>
/// Tool execution result. Capability deny uses ErrorKind = "CapabilityDenied"
/// and never invokes the tool handler (RFC 038 lock).
/// IsBufferedArgs = true 时，本结果不是最终执行结果，而是 Buffer 流式路径
/// 的"完整参数就绪"标记（Content 承载拼好的 args JSON），由 Session 调度
/// 异步执行——不复用 ErrorKind 承载流标记（错误语义与流标记分离）。
/// </summary>
public class AIToolResult {
    public string CallId;
    public string Content;
    public bool IsError;
    public string ErrorKind;
    public bool IsBufferedArgs;

    public AIToolResult() {
        this.CallId = "";
        this.Content = "";
        this.IsError = false;
        this.ErrorKind = "";
        this.IsBufferedArgs = false;
    }

    public AIToolResult(string callId, string content, bool isError) {
        this.CallId = callId != null ? callId : "";
        this.Content = content != null ? content : "";
        this.IsError = isError;
        this.ErrorKind = "";
        this.IsBufferedArgs = false;
    }

    public AIToolResult(string callId, string content, bool isError, string errorKind) {
        this.CallId = callId != null ? callId : "";
        this.Content = content != null ? content : "";
        this.IsError = isError;
        this.ErrorKind = errorKind != null ? errorKind : "";
        this.IsBufferedArgs = false;
    }

    public static AIToolResult Ok(string callId, string content) {
        return new AIToolResult(callId, content, false, "");
    }

    public static AIToolResult Fail(string callId, string errorKind, string content) {
        return new AIToolResult(callId, content, true, errorKind);
    }

    public static AIToolResult CapabilityDenied(string callId, string toolName) {
        string name = toolName != null ? toolName : "";
        return AIToolResult.Fail(callId, "CapabilityDenied", "capability denied for tool: " + name);
    }
}
