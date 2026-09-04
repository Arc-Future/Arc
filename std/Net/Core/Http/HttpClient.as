// RFC 025 M4 + RFC 033 S1/§1.0/§1.4: Arc.Net — HttpClient 统一门面（薄门面 · 异步）。
//
// 对齐 C# System.Net.Http.HttpClient（.NET 9）精华：薄门面，不写协议/池/解析，
// 委托给 HttpMessageHandler 链（默认 SocketsHttpHandler → HttpConnectionPool →
// 版本连接）。异步单一惯用法（§1.4 · RFC 028 异步为主）：唯一执行入口为 SendAsync /
// GetAsync 等 Task 面；HTTP/1.1 全链路经 Reactor **真异步**（TCP/TLS 握手与读写 await），
// 不阻塞调用线程（与 WebSocketClient 同例）。HTTP/2/HTTP/3 收敛为内部连接层后
// 由 handler 按 HttpVersionPolicy 版本协商选择（§1.0/§1.2.i）。
//
// 诚实边界：无 HttpListener；无 Connect/SSE 等历史误型（RFC 033 §1.0.1 移除项，不保留）。
// https:// 经 SocketsHttpHandler 的 TLS 传输（TlsClientSession + TlsNetworkStream，RFC 033
// S3 / RFC 026 深层 https（TLS 1.3，见 RFC 025 网络）+ 默认 FullChain + OS 系统根，可连真实公网主机。
namespace Arc.Net;

using Arc.IO;

/// <summary>
/// HTTP 客户端——统一门面（异步 Task 面；HTTP/1.1 当面，HTTP/2/3 版本协商路由）。
///
/// 使用示例：
///   using var http = new HttpClient();
///   http.BaseAddress = new Uri("http://api.example.com");
///   http.Timeout = 10000;
///   var resp = await http.GetAsync("/users");
///   string body = await resp.Content.ReadAsStringAsync();
/// </summary>
public class HttpClient : IDisposable {
    public Uri BaseAddress;
    public int Timeout;
    public int MaxRetries;
    public WebHeaderCollection DefaultRequestHeaders;
    public CookieContainer CookieContainer;

    private HttpMessageHandler _handler;
    // 默认 sockets handler（仅当 _handler 为 SocketsHttpHandler 时非 null）：
    // 承载 Timeout/MaxRetries/连接池等具体传输面转发；自定义 handler（测试/委托链）无此面。
    private SocketsHttpHandler _sockets;

    public HttpClient() {
        this.Timeout = 30000;
        this.MaxRetries = 2;
        // 门面持有一个默认 SocketsHttpHandler（对齐 C# new HttpClient()）。
        SocketsHttpHandler sh = new SocketsHttpHandler();
        _handler = sh;
        _sockets = sh;
        this.DefaultRequestHeaders = sh.DefaultRequestHeaders;
        this.CookieContainer = sh.CookieContainer;
        sh.Timeout = this.Timeout;
        sh.MaxRetries = this.MaxRetries;
    }

    /// <summary>以给定 handler 链构造（对齐 C# HttpClient(HttpMessageHandler)）。
    /// 自定义 handler 不提供 Timeout/MaxRetries/连接池面，仅经 SendAsync 委托发送。</summary>
    public HttpClient(HttpMessageHandler handler) {
        _handler = handler != null ? handler : new SocketsHttpHandler();
        _sockets = null;
        this.Timeout = 30000;
        this.MaxRetries = 2;
        this.DefaultRequestHeaders = new WebHeaderCollection();
        this.CookieContainer = new CookieContainer();
    }

    // ── 核心发送（异步 Task 面）──

    /// <summary>发送 HttpRequestMessage（经由默认 SocketsHttpHandler 链，版本协商路由）。</summary>
    public Task<HttpResponseMessage> SendAsync(HttpRequestMessage req) {
        if (_sockets != null) {
            _sockets.Timeout = this.Timeout;
            _sockets.MaxRetries = this.MaxRetries;
        }
        return _handler.SendAsync(req);
    }

    public Task<HttpResponseMessage> GetAsync(string url) {
        return this.SendAsync(new HttpRequestMessage(HttpMethod.GET, this.ResolveUri(url)));
    }

    public Task<HttpResponseMessage> PostAsync(string url, HttpContent content) {
        return this.SendAsync(new HttpRequestMessage(HttpMethod.POST, this.ResolveUri(url), content));
    }

    public Task<HttpResponseMessage> PostAsync(string url, string body) {
        return this.SendAsync(new HttpRequestMessage(HttpMethod.POST, this.ResolveUri(url), new StringContent(body)));
    }

    public Task<HttpResponseMessage> PutAsync(string url, HttpContent content) {
        return this.SendAsync(new HttpRequestMessage(HttpMethod.PUT, this.ResolveUri(url), content));
    }

    public Task<HttpResponseMessage> PatchAsync(string url, HttpContent content) {
        return this.SendAsync(new HttpRequestMessage(HttpMethod.PATCH, this.ResolveUri(url), content));
    }

    public Task<HttpResponseMessage> DeleteAsync(string url) {
        return this.SendAsync(new HttpRequestMessage(HttpMethod.DELETE, this.ResolveUri(url)));
    }

    public Task<HttpResponseMessage> HeadAsync(string url) {
        return this.SendAsync(new HttpRequestMessage(HttpMethod.HEAD, this.ResolveUri(url)));
    }

    public Task<HttpResponseMessage> OptionsAsync(string url) {
        return this.SendAsync(new HttpRequestMessage(HttpMethod.OPTIONS, this.ResolveUri(url)));
    }

    // ── 便捷取体（异步 Task 面）──

    public async Task<string> GetStringAsync(string url) {
        var resp = await this.SendAsync(new HttpRequestMessage(HttpMethod.GET, this.ResolveUri(url)));
        if (resp == null) { return ""; }
        string b = resp.Body;
        resp.Dispose();
        return b;
    }

    public async Task<byte[]> GetByteArrayAsync(string url) {
        var resp = await this.SendAsync(new HttpRequestMessage(HttpMethod.GET, this.ResolveUri(url)));
        if (resp == null) { return null; }
        byte[] b = resp.Content.ReadAsByteArray();
        resp.Dispose();
        return b;
    }

    /// <summary>请求并返回响应体为流（对齐 C# GetStreamAsync；经 HttpContent.ReadAsStream
    /// 返回 StreamTransport——流式响应为活传输，全缓冲为内存载体）。</summary>
    public async Task<StreamTransport> GetStreamAsync(string url) {
        var resp = await this.SendAsync(new HttpRequestMessage(HttpMethod.GET, this.ResolveUri(url)));
        if (resp == null) { return null; }
        StreamTransport s = resp.Content != null ? resp.Content.ReadAsStream() : null;
        resp.Dispose();
        return s;
    }

    // ── 生命周期 ──

    /// <summary>清空默认 handler 的连接池。</summary>
    public void ClearConnectionPool() {
        if (_sockets != null) {
            _sockets.ClearConnectionPool();
        }
    }

    /// <summary>取消所有待处理请求（对齐 C# CancelPendingRequests；关闭活动连接并清空连接池）。</summary>
    public void CancelPendingRequests() {
        this.ClearConnectionPool();
    }

    public void Dispose() { this.ClearConnectionPool(); }

    // ── Private: URI 解析 ──

    /// <summary>解析 url → Uri；支持 http:// 与 https://（默认端口 80/443）。</summary>
    private Uri ResolveUri(string url) {
        if (url == null) { url = ""; }
        if (url.StartsWith("https://") || url.StartsWith("http://")) { return new Uri(url); }
        if (this.BaseAddress != null) { return new Uri(this.BaseAddress, url); }
        return new Uri("http://" + url);
    }
}