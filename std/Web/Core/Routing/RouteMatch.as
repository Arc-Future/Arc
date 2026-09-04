// RouteMatch —— 路由匹配结果（RFC 040 §1.7 · internal）。
namespace Arc.Web;
using Arc.Collections;

/// <summary>
/// 路由匹配结果（internal）：命中的端点 + 路径参数名/值（对齐模板顺序）。
/// </summary>
internal class RouteMatch {
    public EndpointDescriptor Endpoint;
    public List<string> ParamNames;
    public List<string> ParamValues;

    public RouteMatch(EndpointDescriptor endpoint) {
        Endpoint = endpoint;
        ParamNames = new List<string>();
        ParamValues = new List<string>();
    }
}
