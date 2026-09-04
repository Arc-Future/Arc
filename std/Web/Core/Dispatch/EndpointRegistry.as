// EndpointRegistry —— 端点注册表（RFC 040 §1.7 · internal）。
namespace Arc.Web;
using Arc.Collections;

/// <summary>
/// 端点注册表（internal）：存储全部端点（方法 + 模板 + 分发器），
/// 按 method + path 匹配返回 RouteMatch。
/// </summary>
internal class EndpointRegistry {
    private List<EndpointDescriptor> _endpoints;

    public EndpointRegistry() {
        _endpoints = new List<EndpointDescriptor>();
    }

    public void Add(string method, string template, IEndpointDispatcher dispatcher) {
        EndpointDescriptor ep = new EndpointDescriptor(method, template, dispatcher);
        _endpoints.Add(ep);
    }

    /// <summary>注册端点并声明所需角色（逗号分隔；空串 = 无需鉴权）。</summary>
    public void Add(string method, string template, IEndpointDispatcher dispatcher, string roles) {
        EndpointDescriptor ep = new EndpointDescriptor(method, template, dispatcher, roles);
        _endpoints.Add(ep);
    }

    /// <summary>按 method + path 匹配端点；未命中返回 null。</summary>
    public RouteMatch Match(string method, string path) {
        for (int i = 0; i < _endpoints.Count; i++) {
            EndpointDescriptor ep = _endpoints[i];
            if (ep.Method != method) { continue; }
            RouteMatch m = RouteMatcher.Match(ep, path);
            if (m != null) { return m; }
        }
        return null;
    }

    /// <summary>仅按 path 匹配端点（任意方法）——供宿主区分 404（路径不存在）
    /// 与 405（路径存在但方法不允许，REST 语义）。</summary>
    public RouteMatch MatchAnyMethod(string path) {
        for (int i = 0; i < _endpoints.Count; i++) {
            RouteMatch m = RouteMatcher.Match(_endpoints[i], path);
            if (m != null) { return m; }
        }
        return null;
    }
}
