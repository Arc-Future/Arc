// RFC 025 M4: Arc.Net — HTTP 请求消息。
//
// 对标 C# System.Net.Http.HttpRequestMessage（.NET 9）。
// 封装完整的 HTTP 请求：方法、URI、头、内容。

namespace Arc.Net;

/// <summary>
/// HTTP 请求消息——封装请求方法、URI、头和内容。
///
/// 使用方式：
///   var req = new HttpRequestMessage(HttpMethod.GET, new Uri("http://example.com"));
///   req.Headers.Add("Authorization", "Bearer token");
///   var resp = http.Send(req);
/// </summary>
public class HttpRequestMessage {
    /// <summary>HTTP 方法。</summary>
    public HttpMethod Method;

    /// <summary>请求 URI。</summary>
    public Uri RequestUri;

    /// <summary>请求头集合。</summary>
    public WebHeaderCollection Headers;

    /// <summary>请求体内容。</summary>
    public HttpContent Content;

    /// <summary>原始请求体字符串（兼容简便调用）。</summary>
    public string Body;

    /// <summary>请求 HTTP 版本（如 "HTTP/1.1" / "HTTP/2"）；空则用默认。</summary>
    public string Version;

    /// <summary>版本协商策略（§1.2.i）。</summary>
    public HttpVersionPolicy VersionPolicy;

    /// <summary>流式响应：true 时连接层经 ParseStreaming 返回活动传输，
    /// 消费者以 resp.Content.ReadAsStream()（SSE/大文件增量读）消费。</summary>
    public bool StreamResponse;

    /// <summary>创建空请求消息。</summary>
    public HttpRequestMessage() {
        this.Method = HttpMethod.GET;
        this.Headers = new WebHeaderCollection();
        this.Body = "";
        this.Version = "";
        this.VersionPolicy = HttpVersionPolicy.RequestVersionOrLower;
    }

    /// <summary>创建指定方法和 URI 的请求消息。</summary>
    public HttpRequestMessage(HttpMethod method, Uri uri) {
        this.Method = method;
        this.RequestUri = uri;
        this.Headers = new WebHeaderCollection();
        this.Body = "";
        this.Version = "";
        this.VersionPolicy = HttpVersionPolicy.RequestVersionOrLower;
    }

    /// <summary>创建指定方法、URI 和内容的请求消息。</summary>
    public HttpRequestMessage(HttpMethod method, Uri uri, HttpContent content) {
        this.Method = method;
        this.RequestUri = uri;
        this.Content = content;
        this.Headers = new WebHeaderCollection();
        this.Body = content != null ? content.Body : "";
        this.Version = "";
        this.VersionPolicy = HttpVersionPolicy.RequestVersionOrLower;
    }

    /// <summary>返回请求方法字符串。</summary>
    public string MethodString() {
        if (this.Method == HttpMethod.GET) { return "GET"; }
        if (this.Method == HttpMethod.POST) { return "POST"; }
        if (this.Method == HttpMethod.PUT) { return "PUT"; }
        if (this.Method == HttpMethod.DELETE) { return "DELETE"; }
        if (this.Method == HttpMethod.PATCH) { return "PATCH"; }
        if (this.Method == HttpMethod.HEAD) { return "HEAD"; }
        if (this.Method == HttpMethod.OPTIONS) { return "OPTIONS"; }
        if (this.Method == HttpMethod.CONNECT) { return "CONNECT"; }
        if (this.Method == HttpMethod.TRACE) { return "TRACE"; }
        return "GET";
    }
}
