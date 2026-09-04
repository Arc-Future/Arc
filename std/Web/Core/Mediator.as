// Mediator —— IMediator 唯一实现（RFC 040 §1.5）· provider 持有者。
// 方案状态（2026-08-14 维护者裁决）：泛型分发经 MediatorExtensions 静态扩展承载的方案
// 已取消并迁移完成（commit d7b5900f）——本类直接实现接口泛型方法 SendAsync/PublishAsync
// （MediatorExtensions 已删除，不留双轨）。裁决详见
// docs/rfc/proposals/mediator-generic-dispatch-verdict.md。
// 泛型分发逻辑：SendAsync 经 DI 解析单 handler + 按注册顺序逐层穿 IPipelineBehavior
// 行为链（索引 0 最外层）；PublishAsync 经 DI 解析全部通知 handler 广播（void）。
// Scoped 注册 → 每请求作用域内解析。
namespace Arc.Web;
using Arc;

/// <summary>
/// 中介者实现（RFC 040 §1.5）：持有 DI provider，作为 IMediator 的注册载体。
/// 泛型分发（SendAsync / PublishAsync）经 Provider 按闭包类型
/// （IRequestHandler&lt;,&gt; / IPipelineBehavior&lt;,&gt; / INotificationHandler&lt;&gt;）解析——
/// 行为链与 handler 共享同一 DI 作用域，scoped 服务（DbContext、UnitOfWork 等）在整条管道内一致。
/// </summary>
public class Mediator : IMediator
{
    private IServiceProvider _provider;

    public Mediator(IServiceProvider provider)
    {
        _provider = provider;
    }

    public IServiceProvider Provider
    {
        get
        {
            // CD-29：字段 getter 裸返回是「借用」——命名局部（`this.Provider`）按
            // 新引用 dec 会使 _provider（root 容器）每请求漂移 → 提前 free →
            // 后续 scope.CreateScope/GetService UAF（ARC_DBG_FREE 实证）。经中间
            // 局部赋值转新引用：调用方 dec 独立于字段持有，生命周期平衡。
            object? provider = _provider;
            return (IServiceProvider)provider;
        }
    }

    /// <summary>分发单条请求：单 handler + 行为链（注册序逐层穿管；末环即 handler）。</summary>
    public Task<TResponse> SendAsync<TRequest, TResponse>(TRequest request, CancellationToken cancellationToken)
        where TRequest : IRequest<TResponse>
    {
        IServiceProvider provider = this.Provider;
        IRequestHandler<TRequest, TResponse> handler =
            (IRequestHandler<TRequest, TResponse>)provider.GetService(typeof(IRequestHandler<TRequest, TResponse>));
        List<object?> behaviorObjects =
            provider.GetServices(typeof(IPipelineBehavior<TRequest, TResponse>));
        BehaviorChain<TRequest, TResponse> chain =
            new BehaviorChain<TRequest, TResponse>(request, handler, behaviorObjects, cancellationToken);
        return chain.NextAsync();
    }

    /// <summary>广播通知：全部 INotificationHandler 按注册顺序触发（void）。</summary>
    public Task PublishAsync<TNotification>(TNotification notification, CancellationToken cancellationToken)
        where TNotification : INotification
    {
        IServiceProvider provider = this.Provider;
        List<object?> handlerObjects =
            provider.GetServices(typeof(INotificationHandler<TNotification>));
        for (int i = 0; i < handlerObjects.Count; i++)
        {
            INotificationHandler<TNotification> handler =
                (INotificationHandler<TNotification>)handlerObjects[i];
            handler.HandleAsync(notification, cancellationToken);
        }
        return Task.CompletedTask;
    }
}

/// <summary>
/// 行为链游标（internal）：以方法组递归替代 lambda 链装配——行为按注册序逐层穿管
/// （索引 0 最外层，最先注册最先执行），末环即 handler。
/// </summary>
internal class BehaviorChain<TRequest, TResponse>
{
    private TRequest _request;
    private IRequestHandler<TRequest, TResponse> _handler;
    private List<object?> _behaviors;
    private CancellationToken _cancellationToken;
    private int _index;

    public BehaviorChain(TRequest request, IRequestHandler<TRequest, TResponse> handler, List<object?> behaviors,
        CancellationToken cancellationToken)
    {
        _request = request;
        _handler = handler;
        _behaviors = behaviors;
        _cancellationToken = cancellationToken;
        _index = 0;
    }

    public Task<TResponse> NextAsync()
    {
        if (_index >= _behaviors.Count)
        {
            return _handler.HandleAsync(_request, _cancellationToken);
        }
        IPipelineBehavior<TRequest, TResponse> behavior =
            (IPipelineBehavior<TRequest, TResponse>)_behaviors[_index];
        _index++;
        return behavior.HandleAsync(_request, NextAsync, _cancellationToken);
    }
}
