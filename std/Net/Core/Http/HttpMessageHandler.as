// RFC 033 §1.0/§1.4: Arc.Net — HTTP 消息处理器抽象（对齐 C# HttpMessageHandler）。
//
// 发送抽象：HttpClient 门面不直接写传输，而是委托给 HttpMessageHandler 链
// （默认 SocketsHttpHandler）。异步单一惯用法（§1.4）：唯一执行入口为
// SendAsync（Task 面）；实现在 P1 同步传输原语上以 Task.FromResult 包裹
// （与 WebSocketClient 同例，真异步待 Reactor 异步面）。语言缺口：
// 虚属性返回新对象会 AV，故只用抽象方法发送。
namespace Arc.Net;

/// <summary>
/// HTTP 消息处理器抽象——将 HttpRequestMessage 发送并返回 HttpResponseMessage。
/// 对齐 C# System.Net.Http.HttpMessageHandler。实现者保证连接复用/协议/重试等。
/// </summary>
public abstract class HttpMessageHandler {
    /// <summary>异步发送请求并返回响应（Task 面；P1 同步传输 Task.FromResult 包裹）。</summary>
    public abstract Task<HttpResponseMessage> SendAsync(HttpRequestMessage request);
}