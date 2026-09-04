// RequestContext —— 请求上下文（RFC 040 §1.11）：收窄 HTTP 上下文，无 HttpContext 上帝对象。
// 每请求构造，由宿主填充后：认证方经 UserPrincipal 注入主体；角色鉴权消费。
namespace Arc.Web;
using Arc;
using Arc.Net;

/// <summary>
/// 请求上下文（RFC 040 §1.11）：收窄跨界上下文为显式数据面——方法/路径/查询串/请求头
/// + 每请求服务作用域 + 认证主体。无 HttpContext 上帝对象 / IHttpContextAccessor。
/// 由宿主每请求构造并填充；认证方据此解析主体，鉴权行为据此裁决。
/// </summary>
public class RequestContext {
    /// <summary>HTTP 请求方法（GET/POST/PUT/DELETE/PATCH）。</summary>
    public string Method;

    /// <summary>请求路径（不含查询串）。</summary>
    public string Path;

    /// <summary>查询串（'?' 之后原文；无查询串为空串）。</summary>
    public string Query;

    /// <summary>请求头集合。</summary>
    public WebHeaderCollection Headers;

    /// <summary>每请求服务作用域（供鉴权/处理器解析 Scoped 服务）。</summary>
    public IServiceProvider Services;

    /// <summary>认证主体（null 表示未认证/匿名）。由认证方在请求到达时填充。</summary>
    public UserPrincipal? User;

    public RequestContext() {
        this.Method = "";
        this.Path = "";
        this.Query = "";
        this.Headers = null;
        this.Services = null;
        this.User = null;
    }
}
