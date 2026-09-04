// IAuthorizationBehavior —— 可扩展鉴权行为契约（RFC 040 §1.10）。
namespace Arc.Web;
using Arc;

/// <summary>
/// 可扩展鉴权行为契约（RFC 040 §1.10）：默认角色鉴权（RoleAuthorizationBehavior），
/// 可扩展动态鉴权 / API Key / OAuth Scope 等其他鉴权行为。宿主于分发前调用；
/// 不满足即抛 UnauthorizedException（宿主映射 HTTP 401）。
/// </summary>
public interface IAuthorizationBehavior {
    /// <summary>对一次请求执行鉴权。不满足时抛 UnauthorizedException。</summary>
    /// <param name="context">请求上下文（含认证主体 User）。</param>
    /// <param name="requiredRoles">端点声明角色（逗号分隔；空串表示无需鉴权）。</param>
    Task AuthorizeAsync(RequestContext context, string requiredRoles);
}
