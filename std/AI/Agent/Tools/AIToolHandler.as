// RFC 038: tool handler storage base (mirror AIChatClient — class ref, not interface field).
namespace Arc.Agent;

using Arc;

/// <summary>
/// Tool implementation. Stored as class refs in AIToolSet (same AV workaround as AIChatClient).
/// Async-first contract (RFC 038)：唯一执行入口为 <see cref="InvokeAsync"/>，
/// 无同步孪生（禁同步便利双轨）。同步工具方法由声明式生成代码以 Task.FromResult 适配。
/// Note: no virtual Descriptor getter — virtual property returning new object AVs; AIToolSet builds descriptor from Name/Capability.
/// </summary>
public abstract class AIToolHandler {
    public abstract string Name { get; }
    public abstract string Capability { get; }
    public abstract Task<AIToolResult> InvokeAsync(AIToolCall call, CancellationToken cancellationToken);
}
