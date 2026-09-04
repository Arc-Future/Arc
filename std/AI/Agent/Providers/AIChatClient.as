// RFC 038: storage base for chat-client slot.
// Arc currently AVs when invoking async methods through an interface-typed *instance field*;
// dialects extend this class (and implement IAIChatClient) so Host/Session can store a class ref.
namespace Arc.Agent;

using Arc;
using Arc.Collections;

/// <summary>
/// ChatClient 存储基类。公开契约是 <see cref="IAIChatClient"/>。
/// Host/Session 持有本类引用以避免 interface-field 异步调用运行时 AV。
/// </summary>
// NOTE: kept public — extended by Arc.Agent.DeepSeek across package boundaries.
public abstract class AIChatClient : IAIChatClient {
    public abstract Task<AIReply> CompleteAsync(AIRequest request, CancellationToken cancellationToken);
    public abstract IAsyncEnumerable<AIStreamEvent> StreamEventsAsync(AIRequest request, CancellationToken cancellationToken);
}
