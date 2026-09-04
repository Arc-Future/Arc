namespace Arc.Agent;
using Arc;
/// <summary>
/// Tool-arg stream handler. OnToolArgDelta returns void（编译器已支持 void-virtual
/// 分派，不再用 int 规避——A2 根因已修）。
/// </summary>
public interface IAIToolStreamHandler {
    AIToolStreamDisposition OnToolCallStart(AIToolCallStart start, CancellationToken cancellationToken);
    void OnToolArgDelta(AIToolArgDelta delta, CancellationToken cancellationToken);
    AIToolResult OnToolCallEnd(AIToolCallEnd end, CancellationToken cancellationToken);
}
