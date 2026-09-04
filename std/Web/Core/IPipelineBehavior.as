// IPipelineBehavior —— 唯一横切管道（RFC 040 §1.6）：鉴权/校验/日志/事务/缓存。
namespace Arc.Web;
using Arc;

/// <summary>
/// 唯一横切面：中间件/过滤器/校验/鉴权/日志/事务/缓存全部收编为行为链，无双轨。
/// 按注册顺序穿管：鉴权 → 校验 → 日志 → 事务 → handler。
///
/// next 即「后续管道 + handler」——以 Arc 内建委托类型 Func&lt;Task&lt;TResponse&gt;&gt; 承载
/// （Arc 无 delegate 关键字，故 RequestHandlerDelegate&lt;T&gt; 由 Func&lt;Task&lt;T&gt;&gt; 实化）。
/// </summary>
public interface IPipelineBehavior<TRequest, TResponse> {
    Task<TResponse> HandleAsync(TRequest request, Func<Task<TResponse>> next, CancellationToken cancellationToken);
}
