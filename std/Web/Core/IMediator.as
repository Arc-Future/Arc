// IMediator —— 唯一请求入口（RFC 040 §1.5）。
// 方案状态（2026-08-14 维护者裁决）：泛型分发经 MediatorExtensions 静态扩展承载的方案
// 已取消并迁移完成（commit d7b5900f）——接口直接声明 SendAsync/PublishAsync 泛型方法
// （Mediator 直接实现，MediatorExtensions 已删除，不留双轨）。裁决详见
// docs/rfc/proposals/mediator-generic-dispatch-verdict.md。
namespace Arc.Web;
using Arc;

/// <summary>
/// 中介者：唯一请求入口，也是唯一横切面（IPipelineBehavior）的载体。
/// - 泛型入口 SendAsync&lt;TRequest,TResponse&gt; / PublishAsync&lt;TNotification&gt;
///   由接口直接声明，应用内调用与 HTTP 端点共用（单一惯用法）。
/// - Provider：非泛型底层面，供实现按闭包类型解析 handler 与行为链
///   （IRequestHandler&lt;,&gt; / IPipelineBehavior&lt;,&gt; / INotificationHandler&lt;&gt;）。
/// </summary>
public interface IMediator
{
    /// <summary>DI 服务提供者：按闭包类型解析 handler 与行为链的底层面。</summary>
    IServiceProvider Provider { get; }

    /// <summary>
    /// 分发单条请求：DI 解析单 handler，按注册顺序逐层穿 IPipelineBehavior 行为链后穿管。
    /// 请求类型须实现 <see cref="IRequest{TResponse}"/>，返回强类型 TResponse。
    /// </summary>
    /// <typeparam name="TRequest">命令/查询请求类型。</typeparam>
    /// <typeparam name="TResponse">响应类型。</typeparam>
    /// <param name="request">请求实例。</param>
    /// <param name="cancellationToken">取消令牌。</param>
    /// <returns>handler 最终产生的响应任务。</returns>
    Task<TResponse> SendAsync<TRequest, TResponse>(TRequest request, CancellationToken cancellationToken)
        where TRequest : IRequest<TResponse>;

    /// <summary>
    /// 广播通知：DI 解析全部 INotificationHandler 并按注册顺序触发（void）。
    /// 通知类型须实现 <see cref="INotification"/>。
    /// </summary>
    /// <typeparam name="TNotification">通知类型。</typeparam>
    /// <param name="notification">通知实例。</param>
    /// <param name="cancellationToken">取消令牌。</param>
    /// <returns>已完成的任务。</returns>
    Task PublishAsync<TNotification>(TNotification notification, CancellationToken cancellationToken)
        where TNotification : INotification;
}
