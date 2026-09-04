// RFC 033 §1.0: Arc.Net — SocketsHttpHandler（对齐 C# SocketsHttpHandler）。
//
// 具体传输 handler：编排 连接池(HttpConnectionPool) + Cookie 注入/Cookie 收集 +
// 重试 + 版本连接选择。HTTP/1.1 → Http11Connection；HTTP/2 → 内部 Http2Connection
// （物理收敛，§1.0 ①/§1.2.i，按 HttpVersionPolicy + 请求 Version 路由并桥接统一
// HttpRequestMessage/HttpResponseMessage）。HTTP/3 受 QUIC 传输语言缺口约束，
// 待 rt_quic_* 接入后再按 Alt-Svc 协商收敛（见 Http3 内部连接层 Qpack/Http3Frame 诚实边界）。
// 逻辑由原 HttpClient.as 的 Send 迁移至此（单一职责）。异步统一面（§1.4 · RFC 028
// 异步为主）：唯一执行入口 SendAsync（Task 面）为**真异步**——HTTP/1.1 全链路经
// Reactor 真异步（TcpClient.ConnectAsync/SendAsync/ReceiveAsync 与
// TlsClientSession.AuthenticateAsClientAsync），不阻塞调用线程；HTTP/2 走内部
// Http2Connection 同步面（物理收敛，非 SSE/DeepSeek 消费路径）。
namespace Arc.Net;

using Arc.Net.Security;
using Arc.Security.Cryptography;
using Arc.Text;

/// <summary>
/// 套接字传输 handler——发送 HttpRequestMessage 并返回 HttpResponseMessage。
/// 对齐 C# System.Net.Http.SocketsHttpHandler。默认由 HttpClient 持有。
/// </summary>
public class SocketsHttpHandler : HttpMessageHandler {
    public HttpConnectionPool Pool;
    public CookieContainer CookieContainer;
    public WebHeaderCollection DefaultRequestHeaders;
    public int Timeout;
    public int MaxRetries;

    // HTTPS 证书校验（对齐 C# HttpClientHandler.ServerCertificateCustomValidationCallback 面）。
    // 默认（_trustAnchors 为空）走 TlsClientSession 默认 FullChain + OS 系统根——真实公网
    // 主机（如 DeepSeek 的 api.deepseek.com）证书链可校验；设置 _trustAnchors 后按
    // VerifyMode + 显式锚链校验（本地自签测试证书场景）。
    public TlsCertificateVerification VerifyMode;
    public List<X509Certificate2> TrustAnchors;

    // HTTP/2 内部连接层（复用单一连接承载多流；按 authority 隔离）。
    private Http2Connection _h2;
    private string _h2Host;
    private int _h2Port;

    public SocketsHttpHandler() {
        this.Pool = new HttpConnectionPool();
        this.CookieContainer = new CookieContainer();
        this.DefaultRequestHeaders = new WebHeaderCollection();
        this.Timeout = 30000;
        this.MaxRetries = 2;
        this.VerifyMode = TlsCertificateVerification.FullChain;
        this.TrustAnchors = new List<X509Certificate2>();
        _h2 = null;
        _h2Host = "";
        _h2Port = 0;
    }

    /// <summary>异步发送请求（真异步 · RFC 028 异步为主）。HTTP/1.1 全链路经
    /// Reactor 真异步（TcpClient.ConnectAsync/SendAsync/ReceiveAsync 与
    /// TlsClientSession.AuthenticateAsClientAsync），不阻塞调用线程；HTTP/2 走内部
    /// Http2Connection 同步面（物理收敛，非 SSE/DeepSeek 消费路径）。</summary>
    public override async Task<HttpResponseMessage> SendAsync(HttpRequestMessage request) {
        var uri = request.RequestUri;
        if (uri == null) { return null; }

        // 版本路由：HTTP/2 请求 → 内部 Http2Connection（物理收敛，§1.2.i）。
        if (this.WantsHttp2(request)) {
            return this.SendHttp2Sync(request, uri);
        }

        // ── HTTP/1.1 路径（真异步）──
        bool isHttps = uri.Scheme == "https";
        var cl = this.Pool.AcquireConnection(uri.Host, uri.Port);
        if (cl == null) { return null; }

        var conn = await this.BuildHttp11Async(cl, uri, isHttps);
        if (conn == null) { cl.Close(); return null; }

        string body = "";
        string ct = "";
        if (request.Content != null) {
            body = request.Content.Body;
            ct = request.Content.ContentType;
        } else if (request.Body != "") {
            body = request.Body;
        }

        // 统一头注入（Cookie / 请求头 / 默认头）：定位头体分隔 "\r\n\r\n" 前插入。
        // 旧 `Length-2` 定位在 POST 带体时落进 body 内损坏请求，改为分隔符定位（体完整保持）。
        string reqStr = Http11Connection.BuildRequest(request.MethodString(), uri.AbsolutePath, uri.Host, body, ct);
        string cookies = this.CookieContainer.GetCookieHeader(uri);
        string reqHeaders = request.Headers.ToHeaderString();
        string defaultHeaders = this.DefaultRequestHeaders.ToHeaderString();
        string inject = "";
        if (cookies != "") { inject = "Cookie: " + cookies; }
        if (reqHeaders != "") {
            if (inject != "") { inject = inject + "\r\n"; }
            inject = inject + reqHeaders;
        }
        if (defaultHeaders != "") {
            if (inject != "") { inject = inject + "\r\n"; }
            inject = inject + defaultHeaders;
        }
        reqStr = SocketsHttpHandler.InjectHeaders(reqStr, inject);

        // MaxRetries=2 表示最多重试 2 次（共 3 次尝试：1 首次 + 2 重试）。
        for (int attempt = 0; attempt <= this.MaxRetries; attempt++) {
            bool sent = await conn.SendAsync(reqStr);
            bool connected = conn.Connected;
            if (connected || attempt >= this.MaxRetries) {
                break;
            }
            conn.Close();
            var nc = new TcpClient();
            nc.SetReceiveTimeout(this.Timeout);
            nc.SetSendTimeout(this.Timeout / 3);
            nc.SetNoDelay(true);
            await nc.ConnectAsync(uri.Host, uri.Port);
            if (!nc.Connected) { nc.Close(); return null; }
            conn = await this.BuildHttp11Async(nc, uri, isHttps);
            if (conn == null) { nc.Close(); return null; }
        }
        if (!conn.Connected) { conn.Close(); return null; }

        // 流式请求 → ParseStreamingAsync（不消费体，暴露活传输）；否则全缓冲 ParseAsync。
        bool streaming = request.StreamResponse;
        HttpResponseMessage resp = null;
        if (streaming) {
            resp = await conn.ParseStreamingAsync(request.MethodString());
        } else {
            resp = await conn.ParseAsync(request.MethodString());
        }
        if (resp == null) { conn.Close(); return null; }

        // Collect Set-Cookie。
        string setCookie = resp.Headers.Get("Set-Cookie");
        if (setCookie != "") {
            this.CookieContainer.Add(uri, setCookie);
        }

        resp._host = uri.Host;
        resp._port = uri.Port;
        resp._connection = conn.Tcp;
        if (streaming) {
            // 流式：体未排水，连接不复用（调用方消费后自行 Close）。
            this.Pool.ClearConnectionPool();
        } else {
            bool keep = this.Pool.ShouldKeepAlive(resp);
            if (keep) {
                this.Pool.StorePool(uri.Host, uri.Port, conn.Tcp);
            } else {
                this.Pool.ClearConnectionPool();
            }
        }
        return resp;
    }

    /// <summary>按 scheme 异步构建 HTTP/1.1 连接：http → NetworkStream；https →
    /// TLS 真异步握手（<see cref="TlsClientSession.AuthenticateAsClientAsync"/>）+ TlsNetworkStream。</summary>
    private async Task<Http11Connection> BuildHttp11Async(TcpClient cl, Uri uri, bool isHttps) {
        StreamTransport transport = null;
        if (isHttps) {
            NetworkStream raw = new NetworkStream(cl, this.Timeout);
            TlsClientSession tls = new TlsClientSession(raw);
            tls.TargetHost = uri.Host;
            // HTTP/1.1 客户端仅协商 http/1.1：默认 ALPN 含 h2，公网服务器（如
            // Cloudflare）会选 h2 并以 HTTP/2 SETTINGS 帧应答，HTTP/1.1 解析失败。
            List<string> alpn = new List<string>();
            alpn.Add("http/1.1");
            tls.ApplicationProtocols = alpn;
            // 显式信任锚：本地自签测试证书场景按 VerifyMode + 锚链校验；
            // 空锚 → 保持 TlsClientSession 默认（FullChain + OS 系统根，真实公网主机）。
            if (this.TrustAnchors != null && this.TrustAnchors.Count > 0) {
                tls.VerifyMode = this.VerifyMode;
                for (int ai = 0; ai < this.TrustAnchors.Count; ai++) {
                    tls.TrustAnchors.Add(this.TrustAnchors[ai]);
                }
            }
            try {
                await tls.AuthenticateAsClientAsync();
            } catch (Exception ex) {
                raw.Close();
                return null;
            }
            if (!tls.IsAuthenticated) {
                raw.Close();
                return null;
            }
            transport = new TlsNetworkStream(tls);
        } else {
            transport = new NetworkStream(cl, this.Timeout);
        }
        return new Http11Connection(transport, cl, this.Timeout);
    }

    /// <summary>是否按 HTTP/2 路由：显式 Version=HTTP/2，或协商策略倾向更高版本。</summary>
    private bool WantsHttp2(HttpRequestMessage request) {
        string v = request.Version;
        if (v == "HTTP/2" || v == "2" || v == "2.0") { return true; }
        // RequestVersionOrHigher：优先 HTTP/2（HTTP/1.1 请求默认即此策略，但仅当
        // 显式请求 HTTP/2 版本时路由；Version 为空视为 HTTP/1.1 当面）。
        HttpVersionPolicy p = request.VersionPolicy;
        if (p == HttpVersionPolicy.RequestVersionExact && v == "HTTP/2") { return true; }
        return false;
    }

    /// <summary>HTTP/2 物理收敛路径：按 authority 复用内部 Http2Connection，桥接统一消息。</summary>
    private HttpResponseMessage SendHttp2Sync(HttpRequestMessage request, Uri uri) {
        // 按 authority 建立/复用连接（复用单一连接承载多流）。
        if (_h2 == null || !_h2.Connected || _h2Host != uri.Host || _h2Port != uri.Port) {
            if (_h2 != null) { _h2.Close(); }
            Http2Connection c = new Http2Connection();
            if (!c.Connect(uri.Host, uri.Port)) { return null; }
            _h2 = c;
            _h2Host = uri.Host;
            _h2Port = uri.Port;
        }

        // 桥接：HttpRequestMessage → Http2Request。
        Http2Request h2req = new Http2Request(request.MethodString(), uri.PathAndQuery);
        this.CopyRequestHeaders(request, h2req);
        HttpContent content = request.Content;
        if (content != null && content.Body != "") {
            h2req.Body = Encoding.GetBytes(content.Body);
        } else if (request.Body != "") {
            h2req.Body = Encoding.GetBytes(request.Body);
        }

        Http2Response h2resp = _h2.SendRequest(h2req);
        if (h2resp == null) { return null; }

        // 桥接：Http2Response → HttpResponseMessage。
        HttpResponseMessage resp = new HttpResponseMessage();
        resp.Version = "HTTP/2";
        resp.StatusCode = h2resp.StatusCode;
        resp.ReasonPhrase = "";
        int i = 0;
        while (i < h2resp.Headers.Count) {
            string name = h2resp.Headers.GetName(i);
            string value = h2resp.Headers.GetValue(i);
            if (name.Length > 0 && name[0] == ':') {
                if (name == ":status") { i = i + 1; continue; }
            }
            resp.Headers.Add(name, value);
            i = i + 1;
        }
        resp.Body = h2resp.Body;
        // 内容体：以 StringContent 承载（对齐 HttpResponseMessage.Content 契约，
        // 与 HTTP/1.1 路径同取响应 Content-Type）。
        resp.Content = new StringContent(h2resp.Body, h2resp.Headers.Get("content-type"));
        if (h2resp.Failure != "" && h2resp.StatusCode <= 0) {
            resp.StatusCode = 0;
        }
        resp._host = uri.Host;
        resp._port = uri.Port;
        resp._keepAlive = true;
        return resp;
    }

    /// <summary>把统一请求头（含默认头）拷入 Http2 头列表（跳过 Host/伪头，由连接层统一构造）。</summary>
    private void CopyRequestHeaders(HttpRequestMessage request, Http2Request h2req) {
        string raw = request.Headers.ToHeaderString();
        this.AppendHeadersToHttp2(raw, h2req);
        string extra = this.DefaultRequestHeaders.ToHeaderString();
        this.AppendHeadersToHttp2(extra, h2req);
    }

    private void AppendHeadersToHttp2(string raw, Http2Request h2req) {
        if (raw == "") { return; }
        int pos = 0;
        int len = raw.Length;
        while (pos < len) {
            int lineEnd = raw.IndexOf("\r\n", pos);
            string line = "";
            if (lineEnd < 0) {
                line = raw.Substring(pos, len - pos);
                pos = len;
            } else {
                line = raw.Substring(pos, lineEnd - pos);
                pos = lineEnd + 2;
            }
            int colon = line.IndexOf(": ");
            if (colon > 0) {
                string name = line.Substring(0, colon);
                string value = line.Substring(colon + 2, line.Length - colon - 2);
                // Host 由连接层 BuildRequestHeaders 统一重写为 :authority，跳过用户 Host。
                if (name.ToLower() != "host") {
                    h2req.Headers.Add(name, value);
                }
            }
        }
    }

    /// <summary>关闭 HTTP/2 复用连接（清空连接池时一并释放）。</summary>
    private void CloseH2() {
        if (_h2 != null) {
            _h2.Close();
            _h2 = null;
        }
        _h2Host = "";
        _h2Port = 0;
    }

    /// <summary>把头文本插入请求头体分隔（"\r\n\r\n"）之前，保持请求体完整。</summary>
    private static string InjectHeaders(string reqStr, string headers) {
        if (headers == "") { return reqStr; }
        int sep = reqStr.IndexOf("\r\n\r\n");
        if (sep < 0) { return reqStr; }
        return reqStr.Substring(0, sep) + "\r\n" + headers + reqStr.Substring(sep, reqStr.Length - sep);
    }

    /// <summary>清空连接池。</summary>
    public void ClearConnectionPool() {
        this.CloseH2();
        this.Pool.ClearConnectionPool();
    }
}