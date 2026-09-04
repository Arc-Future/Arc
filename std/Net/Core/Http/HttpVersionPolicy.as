// RFC 033 §1.0.1/§1.2.i: Arc.Net — HTTP 版本协商策略（对齐 C# HttpVersionPolicy）。
//
// 单一协商模型：HttpRequestMessage.Version + HttpClient.DefaultRequestVersion +
// HttpVersionPolicy 决定 SocketsHttpHandler 内部如何选择版本连接（HTTP/1.1 /
// 内部 Http2Connection / Http3Connection，§1.0 ①）。当面默认 HTTP/1.1；
// HTTP/2/3 的 ALPN/Alt-Svc 自动协商随对应内部连接接入后生效。
namespace Arc.Net;

/// <summary>
/// HTTP 版本协商策略（对齐 C# System.Net.Http.HttpVersionPolicy）。
/// </summary>
public enum HttpVersionPolicy {
    /// <summary>请求版本或更低版本（默认；HTTP/1.1 时即 HTTP/1.1）。</summary>
    RequestVersionOrLower = 0,
    /// <summary>请求版本或更高版本（优先 HTTP/2/3）。</summary>
    RequestVersionOrHigher = 1,
    /// <summary>严格使用请求版本，不协商。</summary>
    RequestVersionExact = 2,
}