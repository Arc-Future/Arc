// RFC 033 §1.0/§1.4: Arc.Net — 委托处理器（中间件基类，对齐 C# DelegatingHandler）。
//
// 可组合中间件管线：认证/重试/日志/压缩等通过持有 InnerHandler 串联。
// 异步单一惯用法（§1.4）：SendAsync 为唯一执行入口。
namespace Arc.Net;

/// <summary>
/// 委托处理器基类——把请求转交给内部处理器（InnerHandler），用于构建可组合
/// 中间件链。对齐 C# System.Net.Http.DelegatingHandler。
/// </summary>
public abstract class DelegatingHandler : HttpMessageHandler {
    /// <summary>内部处理器（链上下一环；末端为 SocketsHttpHandler）。</summary>
    public HttpMessageHandler InnerHandler;

    public DelegatingHandler() {
        this.InnerHandler = null;
    }

    public DelegatingHandler(HttpMessageHandler innerHandler) {
        this.InnerHandler = innerHandler;
    }

    /// <summary>转交内部处理器；无内部处理器时返回空响应。</summary>
    public override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request) {
        if (this.InnerHandler != null) {
            return this.InnerHandler.SendAsync(request);
        }
        return Task.FromResult(new HttpResponseMessage());
    }
}