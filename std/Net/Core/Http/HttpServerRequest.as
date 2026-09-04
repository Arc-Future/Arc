// Arc.Net — 服务端单请求（请求行 + 头 + 体）。
//
// 与客户端 HttpRequestMessage 对应的服务端请求载体：由 Http11ServerConnection
// 解析填充，供服务端分发（Arc.Web 路由/绑定）消费。
namespace Arc.Net;

/// <summary>
/// 服务端 HTTP 请求：方法 + 路径 + 头集合 + 体（Content-Length 累积）。
/// 供 HTTP 服务端原语（Http11ServerConnection）与上层分发复用。
/// </summary>
public class HttpServerRequest {
    public string Method;
    public string Path;
    public WebHeaderCollection Headers;
    public string Body;

    public HttpServerRequest() {
        Method = "";
        Path = "";
        Headers = new WebHeaderCollection();
        Body = "";
    }
}
