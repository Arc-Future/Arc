// IRequestHandler —— 请求处理器契约（RFC 040 §1.5）：统一 async，强类型返回。
namespace Arc.Web;
using Arc;

/// <summary>
/// 处理器契约：纯业务逻辑（不感知 HTTP），统一 async，返回强类型 TResponse。
/// 一条请求 = 一个处理器（SendAsync 单 handler 分发）。
/// 注：接口级 where 约束（TRequest : IRequest&lt;TResponse&gt;）为 Arc 泛型接口声明
/// 暂不支持（语言缺口），强类型仍由泛型参数保证；约束可留待语言完善后补。
public interface IRequestHandler<TRequest, TResponse> {
    Task<TResponse> HandleAsync(TRequest request, CancellationToken cancellationToken);
}
