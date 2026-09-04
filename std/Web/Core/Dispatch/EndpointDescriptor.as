// EndpointDescriptor —— 端点描述（RFC 040 §1.7 · internal）。
namespace Arc.Web;
using Arc.Collections;

/// <summary>
/// 端点描述（internal）：HTTP 方法 + 路由模板（含解析段）+ 分发器 + 声明角色。
/// 由 WebApplication.MapGet/MapPost 经 EndpointRegistry 注册。
/// Roles 为空串表示无需鉴权（public）；否则经 IAuthorizationBehavior 校验。
/// </summary>
internal class EndpointDescriptor {
    public string Method;
    public string Template;
    public List<string> Segments;
    public IEndpointDispatcher Dispatcher;
    public string Roles;

    public EndpointDescriptor(string method, string template, IEndpointDispatcher dispatcher) {
        Method = method;
        Template = template;
        Dispatcher = dispatcher;
        Segments = RouteMatcher.SplitTemplate(template);
        Roles = "";
    }

    public EndpointDescriptor(string method, string template, IEndpointDispatcher dispatcher, string roles) {
        Method = method;
        Template = template;
        Dispatcher = dispatcher;
        Segments = RouteMatcher.SplitTemplate(template);
        Roles = roles != null ? roles : "";
    }
}
