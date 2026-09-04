// EndpointDispatcher —— 泛型端点分发器（RFC 040 §1.7 · internal）。
// 以泛型类实现非泛型 IEndpointDispatcher（单一惯用法：注册表统一持有，分发型具体化）。
// 经 IMediator.SendAsync 分发——HTTP 端点与应用内调用走同一条管道（单一惯用法）。
namespace Arc.Web;
using Arc;
using Arc.Text.Json;

/// <summary>
/// 泛型端点分发器（internal）：绑定 JSON → 构造 TRequest → 经 IMediator.SendAsync
/// 分发（DI 解析 handler + IPipelineBehavior 行为链）→ 返回响应对象。IWebResult 桥接与
/// JSON 回退由宿主 HandleRequest 统一负责（本类不做序列化，仅返回对象）。
/// </summary>
internal class EndpointDispatcher<TRequest, TResponse> : IEndpointDispatcher
    where TRequest : Arc.Text.Json.IJsonDeserializable, new() {
    public object Dispatch(DispatchContext ctx) {
        TRequest req = new TRequest();
        JsonSerializer.Deserialize(ctx.BindJson, (IJsonDeserializable)req);
        IMediator mediator = (IMediator)ctx.Sp.GetService(typeof(IMediator));
        Task<TResponse> task = mediator.SendAsync<TRequest, TResponse>(req, CancellationToken.None);
        TResponse resp = task.Result;
        return (object)resp;
    }
}
