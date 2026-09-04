namespace Arc.Agent;
using Arc;
using Arc.Collections;
/// <summary>
/// 模型槽：CompleteAsync（非流式）与 StreamEventsAsync（流式）均为异步。
/// 流式为拉模型异步序列（RFC 008 IAsyncEnumerable 单一惯用法，真异步增量——
/// 每次 MoveNextAsync 驱动一次网络读）；终结事件（Completed/Error）后序列结束。
/// </summary>
public interface IAIChatClient {
    Task<AIReply> CompleteAsync(AIRequest request, CancellationToken cancellationToken);
    IAsyncEnumerable<AIStreamEvent> StreamEventsAsync(AIRequest request, CancellationToken cancellationToken);
}
