// RFC 038: stream handler class base (storage — class refs; void-virtual 已支持)。
namespace Arc.Agent;

using Arc;

/// <summary>
/// Tool-arg stream handler storage base. OnToolArgDelta 返回 void（编译器已支持
/// void-virtual 分派，不再用 int 规避——A2 根因已修）。
/// </summary>
public abstract class AIToolStreamHandler {
    public abstract AIToolStreamDisposition OnToolCallStart(AIToolCallStart start, CancellationToken cancellationToken);
    public abstract void OnToolArgDelta(AIToolArgDelta delta, CancellationToken cancellationToken);
    public abstract AIToolResult OnToolCallEnd(AIToolCallEnd end, CancellationToken cancellationToken);
}
