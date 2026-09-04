// IRequest —— 请求契约（RFC 040 §1.5）：请求类型 = 端点声明。
namespace Arc.Web;

/// <summary>
/// 命令/查询请求契约。请求类型 = 端点声明（路由 + HTTP 方法 + 绑定自描述，
/// 无独立路由配置、无控制器）。可带 [Get]/[Post]/[Put]/[Delete]/[Patch] 特性声明端点。
/// 纯应用内调用（无路由特性）亦成立。
/// </summary>
public interface IRequest<TResponse> {
}
