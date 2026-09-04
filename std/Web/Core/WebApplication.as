// WebApplication —— Arc.Web 单宿主（RFC 040 §1.4 · M-C）。
// 单宿主：装配 DI + 注册端点 + 启动 HTTP 服务。复用 std/DI + Arc.Net HTTP
// 服务端原语（Http11ServerConnection / Http2ServerConnection）+ Arc.Text.Json。
// M-C：accept 与 HTTP/1.1 连接读写全链 Reactor 真异步（RFC 038 M2，无阻塞读占线程池；
// 每连接经默认线程池异步处理，线程复用，连接数可远大于线程数）+ 有界并发
// （MaxConcurrentConnections 背压闸，超出拒绝连接）；每请求作用域 + RequestContext + 鉴权；
// 端点分发经 IMediator。
//
// 监听地址/协议（对齐 ASP.NET Core Kestrel 形态）：`RunAsync()` 无端口参数，端点经
// ListenAnyIP/Localhost/Listen() 声明，或经 `Urls` 解析——来源优先级 env ARC_URLS
// (';' 分隔) > 配置 "Urls" > app.Urls.Add(…) > 默认 http://localhost:5000。
// 诚实边界：HTTP/1.1、HTTP/2 已接入传输；HTTPS(TLS)、HTTP/3（QUIC）与同端口协议
// 探测尚无传输底座，启动时如实报 not_implemented；host/地址级绑定（VIP/回环分派）
// 后置于传输底座，当前经 Start(port) 绑定端口。
namespace Arc.Web;
using Arc;
using Arc.Configuration;
using Arc.DI;
using Arc.Net;
using Arc.Text;
using Arc.Text.Json;
using Arc.Threading;

/// <summary>
/// Arc.Web 单宿主（RFC 040 §1.4）：一次性装配 DI + 注册端点 + 启动。
/// - AddServices：注册领域服务（复用 std/DI IServiceCollection）。
/// - UseConfiguration：显式挂接配置；未挂接时 RunAsync 自动解析
///   appSettings.json + appSettings.{env}.json（ARC_ENV，默认 Production）注册 Singleton。
/// - UseAuthentication：挂接可插拔认证方（token/cookie → UserPrincipal）。
/// - MapGet/MapPost：注册端点（显式模板 + 泛型分发器；可选声明角色）。
/// - Urls：监听地址集合（来源 env ARC_URLS > 配置 "Urls" > app.Urls.Add > 默认 5000）。
/// - ListenAnyIP/Localhost/Listen(string host, int port)（可选 Action&lt;ListenOptions&gt;
///   配置回调设 Protocols）：声明监听端点（对齐 ASP.NET Core Kestrel）。
/// - MaxConcurrentConnections：在途连接上限（默认 256；对齐 Kestrel
///   Limits.MaxConcurrentConnections）；超出时新连接立即关闭（背压拒绝）。
/// - RunAsync：无端口参数；启动所有监听端点（accept 与 HTTP/1.1 读写全链 Reactor
///   真异步，线程复用 + 有界背压），路由匹配 + 认证 + 鉴权 + 绑定 + 经 IMediator
///   分发 + JSON 序列化往返。
/// M-B：MapGet/MapPost 显式注册；特性自动路由由 M-D 编译器里程碑接入。
/// </summary>
public class WebApplication {
    private ServiceCollection _services;
    private EndpointRegistry _endpoints;
    private List<ListenOptions> _listeners;
    private IConfiguration? _configuration;
    private Func<RequestContext, UserPrincipal>? _authenticator;
    private int _maxConcurrentConnections;
    private Semaphore? _connectionSlots;

    /// <summary>监听地址集合（"http://host:port"；未显式 Listen 且无自动来源时使用）。</summary>
    public List<string> Urls;

    public WebApplication() {
        _services = new ServiceCollection();
        _endpoints = new EndpointRegistry();
        _listeners = new List<ListenOptions>();
        Urls = new List<string>();
        _configuration = null;
        _authenticator = null;
        _maxConcurrentConnections = 256;
        _connectionSlots = null;
    }

    /// <summary>
    /// 在途连接上限（默认 256，对齐 Kestrel Limits.MaxConcurrentConnections）。
    /// 在 <see cref="RunAsync"/> 启动时固化（对齐 KestrelServerOptions：启动后不可变）；
    /// 超出上限的新连接立即关闭（背压拒绝，不排队）——连接风暴不会耗尽线程池/内存。
    /// </summary>
    public int MaxConcurrentConnections {
        get { return _maxConcurrentConnections; }
        set { _maxConcurrentConnections = value; }
    }

    /// <summary>装配领域服务（复用 std/DI IServiceCollection）。</summary>
    public void AddServices(Action<IServiceCollection> configure) {
        configure(this._services);
    }

    /// <summary>显式挂接配置（Arc.Configuration.IConfiguration），启动时注册为 Singleton。</summary>
    public void UseConfiguration(IConfiguration configuration) {
        _configuration = configuration;
    }

    /// <summary>挂接认证方（可插拔）：token/cookie → UserPrincipal；返回 null 表示未认证。</summary>
    public void UseAuthentication(Func<RequestContext, UserPrincipal> authenticator) {
        _authenticator = authenticator;
    }

    /// <summary>监听任意 IP（对齐 Kestrel ListenAnyIP）：端口 + 默认 Http1。</summary>
    public void ListenAnyIP(int port) {
        this.AddListener("*", port, null);
    }

    /// <summary>监听任意 IP，经配置回调设置协议等（对齐 Kestrel ListenAnyIP(port, configure)）。</summary>
    public void ListenAnyIP(int port, Action<ListenOptions> configure) {
        this.AddListener("*", port, configure);
    }

    /// <summary>监听本机回环（对齐 Kestrel ListenLocalhost）。</summary>
    public void ListenLocalhost(int port) {
        this.AddListener("localhost", port, null);
    }

    /// <summary>监听本机回环，经配置回调设置协议等。</summary>
    public void ListenLocalhost(int port, Action<ListenOptions> configure) {
        this.AddListener("localhost", port, configure);
    }

    /// <summary>监听指定 host（对齐 Kestrel Listen(IPAddress, port)）。</summary>
    public void Listen(string host, int port) {
        this.AddListener(host, port, null);
    }

    /// <summary>监听指定 host，经配置回调设置协议等。</summary>
    public void Listen(string host, int port, Action<ListenOptions> configure) {
        this.AddListener(host, port, configure);
    }

    /// <summary>统一落点：构造端点并应用配置回调（默认 Http1）。</summary>
    private void AddListener(string host, int port, Action<ListenOptions> configure) {
        ListenOptions opt = new ListenOptions(host, port, HttpProtocols.Http1);
        if (configure != null) { configure(opt); }
        _listeners.Add(opt);
    }

    /// <summary>注册 GET 端点：`{template}` 匹配 path，路径参数绑定请求标量属性（无需鉴权）。</summary>
    public void MapGet<TRequest, TResponse>(string template)
        where TRequest : Arc.Text.Json.IJsonDeserializable, new() {
        this.AddEndpoint<TRequest, TResponse>("GET", template, "");
    }

    /// <summary>注册 GET 端点并声明所需角色（逗号分隔；无匹配角色 → 401）。</summary>
    public void MapGet<TRequest, TResponse>(string template, string roles)
        where TRequest : Arc.Text.Json.IJsonDeserializable, new() {
        this.AddEndpoint<TRequest, TResponse>("GET", template, roles);
    }

    /// <summary>注册 POST 端点：JSON body 绑定请求（无需鉴权）。</summary>
    public void MapPost<TRequest, TResponse>(string template)
        where TRequest : Arc.Text.Json.IJsonDeserializable, new() {
        this.AddEndpoint<TRequest, TResponse>("POST", template, "");
    }

    /// <summary>注册 POST 端点并声明所需角色（逗号分隔；无匹配角色 → 401）。</summary>
    public void MapPost<TRequest, TResponse>(string template, string roles)
        where TRequest : Arc.Text.Json.IJsonDeserializable, new() {
        this.AddEndpoint<TRequest, TResponse>("POST", template, roles);
    }

    /// <summary>注册 PUT 端点：JSON body 绑定请求（无需鉴权）。</summary>
    public void MapPut<TRequest, TResponse>(string template)
        where TRequest : Arc.Text.Json.IJsonDeserializable, new() {
        this.AddEndpoint<TRequest, TResponse>("PUT", template, "");
    }

    /// <summary>注册 PUT 端点并声明所需角色（逗号分隔；无匹配角色 → 401）。</summary>
    public void MapPut<TRequest, TResponse>(string template, string roles)
        where TRequest : Arc.Text.Json.IJsonDeserializable, new() {
        this.AddEndpoint<TRequest, TResponse>("PUT", template, roles);
    }

    /// <summary>注册 DELETE 端点（无需鉴权）。</summary>
    public void MapDelete<TRequest, TResponse>(string template)
        where TRequest : Arc.Text.Json.IJsonDeserializable, new() {
        this.AddEndpoint<TRequest, TResponse>("DELETE", template, "");
    }

    /// <summary>注册 DELETE 端点并声明所需角色（逗号分隔；无匹配角色 → 401）。</summary>
    public void MapDelete<TRequest, TResponse>(string template, string roles)
        where TRequest : Arc.Text.Json.IJsonDeserializable, new() {
        this.AddEndpoint<TRequest, TResponse>("DELETE", template, roles);
    }

    /// <summary>注册 PATCH 端点：JSON body 绑定请求（无需鉴权）。</summary>
    public void MapPatch<TRequest, TResponse>(string template)
        where TRequest : Arc.Text.Json.IJsonDeserializable, new() {
        this.AddEndpoint<TRequest, TResponse>("PATCH", template, "");
    }

    /// <summary>注册 PATCH 端点并声明所需角色（逗号分隔；无匹配角色 → 401）。</summary>
    public void MapPatch<TRequest, TResponse>(string template, string roles)
        where TRequest : Arc.Text.Json.IJsonDeserializable, new() {
        this.AddEndpoint<TRequest, TResponse>("PATCH", template, roles);
    }

    private void AddEndpoint<TRequest, TResponse>(string method, string template, string roles)
        where TRequest : Arc.Text.Json.IJsonDeserializable, new() {
        IEndpointDispatcher dispatcher = (IEndpointDispatcher)new EndpointDispatcher<TRequest, TResponse>();
        _endpoints.Add(method, template, dispatcher, roles);
    }

    /// <summary>
    /// 启动所有已声明监听端点并持续服务请求（await 直至进程终止，对齐 Kestrel Run 阻塞语义）。
    /// 无端口参数——端点经 Listen 系列声明，或经 Urls 解析（优先 env ARC_URLS > 配置 "Urls" >
    /// app.Urls > 默认 http://localhost:5000），对齐 ASP.NET Core `Run()` 由 Urls/Kestrel
    /// 决定监听地址。
    /// P0-b：accept 与连接读写全链 Reactor 真异步（RFC 038 M2）——accept 循环与请求解析
    /// 经 TcpListener.AcceptTcpClientAsync / Http11ServerConnection.ReadRequestAsync /
    /// WriteResponseAsync，无任何阻塞读占用线程池 worker；HTTP/2 连接面仍为同步原语
    /// （诚实边界），经默认线程池卸载执行。
    /// </summary>
    public async Task RunAsync() {
        if (_configuration == null) {
            _configuration = Configuration.Load();
        }
        if (_configuration != null) {
            _services.AddSingleton<IConfiguration>(_configuration);
        }
        _services.AddScoped<IMediator, Mediator>();
        _services.AddSingleton<IAuthorizationBehavior, RoleAuthorizationBehavior>();
        IServiceProvider rootSp = _services.Build();
        // 有界并发背压闸（对齐 Kestrel MaxConcurrentConnections）：计数信号量
        // 初始=上限；accept 非阻塞获取（Wait(0)），失败即拒绝连接。上限在此固化。
        _connectionSlots = new Semaphore(_maxConcurrentConnections, _maxConcurrentConnections);
        if (_listeners.Count == 0) {
            this.SetupEndpointsFromUrls();
        }
        if (_listeners.Count == 0) {
            Console.WriteLine("web_fail:no_endpoint");
            return;
        }
        // 各监听端点经 Reactor 真异步 accept 循环（fire-and-forget async 协程，
        // 不占用专用线程；IO 空闲即挂起，连接到达由 Reactor 唤醒）。
        int i = 0;
        while (i < _listeners.Count) {
            ListenOptions opt = _listeners[i];
            this.ListenLoopAsync(rootSp, opt);
            i = i + 1;
        }
        // 保活：服务器运行至进程终止（对齐 Kestrel Run 阻塞语义——根任务恒挂起，
        // EventLoop 持续驱动 Reactor + 线程池）。
        while (true) {
            await Task.Delay(60000);
        }
    }

    /// <summary>
    /// 未显式 Listen 时从 Urls 解析端点。来源优先级对齐 ASP.NET Core：
    /// 环境变量 ARC_URLS（';' 分隔）> 配置 "Urls" > app.Urls.Add(…) > 默认 http://localhost:5000。
    /// 协议：http → Http1，https → Http2（诚实边界：TLS/HTTPS 未接入时启动报告 not_implemented）。
    /// </summary>
    private void SetupEndpointsFromUrls() {
        List<string> urls = this.ResolveUrls();
        int i = 0;
        while (i < urls.Count) {
            Uri parsed = Uri.TryCreate(urls[i]);
            if (parsed == null) {
                Console.WriteLine("web_fail:bad_url:" + urls[i]);
                return;
            }
            HttpProtocols protocols = parsed.Scheme == "https" ? HttpProtocols.Http2 : HttpProtocols.Http1;
            _listeners.Add(new ListenOptions(parsed.Host, parsed.Port, protocols));
            i = i + 1;
        }
    }

    /// <summary>按优先级解析监听 URL 列表（env > 配置 > app.Urls > 默认 5000）。</summary>
    private List<string> ResolveUrls() {
        string envUrls = Environment.GetEnvironmentVariable("ARC_URLS");
        if (envUrls != null && envUrls != "") {
            return this.ParseUrlList(envUrls);
        }
        if (_configuration != null) {
            string cfgUrls = _configuration.GetValue<string>("Urls");
            if (cfgUrls != null && cfgUrls != "") {
                return this.ParseUrlList(cfgUrls);
            }
        }
        if (this.Urls.Count > 0) {
            return this.Urls;
        }
        List<string> defaults = new List<string>();
        defaults.Add("http://localhost:5000");
        return defaults;
    }

    /// <summary>按 ';' 拆分 URL 列表（空项忽略）。</summary>
    private List<string> ParseUrlList(string list) {
        List<string> urls = new List<string>();
        string[] parts = list.Split(";");
        int i = 0;
        while (i < parts.Length) {
            string u = parts[i];
            if (u != "") { urls.Add(u); }
            i = i + 1;
        }
        return urls;
    }

    /// <summary>
    /// 单端点 Reactor 异步 accept 循环：绑定端口后逐连接派发（accept 与连接处理均
    /// Reactor 真异步，无阻塞线程——accept 单协程非瓶颈，处理不占连接线程，对齐
    /// Kestrel 分层）。连接处理经默认线程池（线程复用 + 有界背压）异步执行。
    /// </summary>
    private async Task ListenLoopAsync(IServiceProvider rootSp, ListenOptions opt) {
        // 诚实边界：HTTPS(TLS)、QUIC、同端口协议探测尚无传输底座，如实报告。
        if (opt.Https) {
            Console.WriteLine("web_fail:https_not_implemented:" + Convert.ToString(opt.Port));
            return;
        }
        if (opt.Protocols == HttpProtocols.Http3 || opt.Protocols == HttpProtocols.Http1AndHttp2AndHttp3) {
            Console.WriteLine("web_fail:http3_not_implemented:" + Convert.ToString(opt.Port));
            return;
        }
        if (opt.Protocols == HttpProtocols.Http1AndHttp2) {
            Console.WriteLine("web_fail:http_negotiate_not_implemented:" + Convert.ToString(opt.Port));
            return;
        }
        TcpListener listener = new TcpListener();
        if (!listener.Start(opt.Port)) {
            Console.WriteLine("web_fail:bind:" + Convert.ToString(opt.Port));
            return;
        }
        Console.WriteLine("web_listening:" + Convert.ToString(opt.Port));
        while (true) {
            // Reactor 真异步 accept（RFC 038 M2）：IO 空闲即挂起协程，连接到达由
            // Reactor 完成唤醒——accept 不阻塞任何线程。
            TcpClient client = await listener.AcceptTcpClientAsync();
            if (client == null) {
                continue;
            }
            // 有界并发背压（对齐 Kestrel MaxConcurrentConnections）：信号量槽位
            // 非阻塞获取，满则立即拒绝（关闭连接，不排队）——连接风暴不会拖垮
            // 线程池/内存。拒绝路径无共享状态；槽位在 ServeConnectionGuardedAsync 的
            // finally 恒归还恰一次。
            if (!_connectionSlots.Wait(0)) {
                client.Close();
                continue;
            }
            // 连接处理异步化（请求读写均 Reactor 真异步，不阻塞 worker；线程复用——
            // 并发连接数可远大于线程数，消除 thread-per-connection 的线程爆炸）。
            this.ServeConnectionGuardedAsync(rootSp, client, opt.Protocols);
        }
    }

    /// <summary>
    /// 线程池 worker 上的连接服务入口：统一在 finally 归还背压槽位（无论
    /// HTTP/1.1 或 HTTP/2 路径、无论正常完成还是异常），保证闸口恒等。
    /// </summary>
    private async Task ServeConnectionGuardedAsync(IServiceProvider rootSp, TcpClient client, HttpProtocols protocols) {
        try {
            await this.ServeConnectionAsync(rootSp, client, protocols);
        } finally {
            _connectionSlots.Release();
        }
    }

    /// <summary>按端点协议分派单连接处理（HTTP/1.1 Reactor 真异步；HTTP/2 同步原语卸载至线程池）。</summary>
    private async Task ServeConnectionAsync(IServiceProvider rootSp, TcpClient client, HttpProtocols protocols) {
        if (protocols == HttpProtocols.Http2) {
            // 诚实边界：HTTP/2 连接面（Http2ServerConnection）仍为同步原语，
            // 经默认线程池卸载执行（对齐既有 M-C 语义），不阻塞异步 accept 续体。
            await Task.Run(() => this.ServeHttp2(rootSp, client));
            return;
        }
        await this.ServeHttp1Async(rootSp, client);
    }

    /// <summary>HTTP/1.1 单连接：异步读请求 → 分发 → 异步写响应（全链 Reactor 真异步）。</summary>
    private async Task ServeHttp1Async(IServiceProvider rootSp, TcpClient client) {
        Http11ServerConnection conn = new Http11ServerConnection(client, 30000);
        HttpServerRequest req = await conn.ReadRequestAsync();
        if (req != null) {
            WebResponse resp = this.HandleRequest(rootSp, req);
            WebHeaderCollection headers = resp.Headers;
            if (headers == null) {
                headers = new WebHeaderCollection();
            }
            headers.Add("X-Arc", "ok");
            await conn.WriteResponseAsync(resp.Status, resp.Reason, headers, resp.ContentType, resp.Body, resp.Data);
        }
        conn.Close();
    }

    /// <summary>HTTP/2 单连接：前置握手 → 读请求 → 适配分发 → 帧写出响应。</summary>
    private void ServeHttp2(IServiceProvider rootSp, TcpClient client) {
        Http2ServerConnection conn = new Http2ServerConnection(client);
        if (!conn.AcceptHandshake()) {
            conn.Close();
            return;
        }
        Http2ServerRequest req = conn.ReadRequest();
        if (req != null) {
            WebResponse resp = this.HandleRequest(rootSp, this.ToHttpServerRequest(req));
            this.WriteHttp2Response(conn, req.StreamId, resp);
            conn.CloseGraceful(req.StreamId);
        } else {
            conn.Close();
        }
    }

    /// <summary>HTTP/2 请求适配为统一分发载体（Http2ServerRequest → HttpServerRequest）。</summary>
    private HttpServerRequest ToHttpServerRequest(Http2ServerRequest req) {
        HttpServerRequest h1 = new HttpServerRequest();
        h1.Method = req.Method;
        h1.Path = req.Path;
        h1.Headers = new WebHeaderCollection();
        Http2HeaderList hs = req.Headers;
        int i = 0;
        while (i < hs.Count) {
            h1.Headers.Add(hs.GetName(i), hs.GetValue(i));
            i = i + 1;
        }
        h1.Body = Encoding.GetString(req.Body);
        return h1;
    }

    /// <summary>HTTP/2 帧写出响应（:status HEADERS + DATA END_STREAM），带 Content-Type 与结果头。</summary>
    private void WriteHttp2Response(Http2ServerConnection conn, int streamId, WebResponse resp) {
        Http2HeaderList headers = new Http2HeaderList();
        headers.Add(":status", Convert.ToString(resp.Status));
        if (resp.ContentType != null && resp.ContentType != "") {
            headers.Add("Content-Type", resp.ContentType);
        }
        if (resp.Headers != null) {
            // 仅回带可列举的关键结果头（WebHeaderCollection 无迭代；按名 Get）。
            string location = resp.Headers.Get("Location");
            if (location != "") {
                headers.Add("Location", location);
            }
        }
        conn.SendResponseHeaders(streamId, headers);
        byte[] body = resp.IsBinary ? resp.Data : Encoding.GetBytes(resp.Body);
        conn.SendData(streamId, body, true);
    }

    /// <summary>处理单个请求：路由匹配 + 认证 + 鉴权 + 经 IMediator 分发。每请求独立作用域。</summary>
    private WebResponse HandleRequest(IServiceProvider rootSp, HttpServerRequest req) {
        IServiceScope scope = rootSp.CreateScope();
        IServiceProvider sp = scope.GetServiceProvider();
        string path = req.Path;
        string query = "";
        int q = path.IndexOf("?");
        if (q >= 0) {
            query = path.Substring(q + 1, path.Length - q - 1);
            path = path.Substring(0, q);
        }
        RouteMatch match = _endpoints.Match(req.Method, path);
        if (match == null) {
            // 路径存在但方法不允许 → 405（对齐 REST 语义）；路径不存在 → 404。
            RouteMatch anyMethod = _endpoints.MatchAnyMethod(path);
            scope.Dispose();
            if (anyMethod != null) {
                return new WebResponse(405, "Method Not Allowed", "{\"error\":\"method_not_allowed\"}");
            }
            return new WebResponse(404, "Not Found", "{\"error\":\"not_found\"}");
        }
        RequestContext context = new RequestContext();
        context.Method = req.Method;
        context.Path = path;
        context.Query = query;
        context.Headers = req.Headers;
        context.Services = sp;
        if (_authenticator != null) {
            context.User = _authenticator(context);
        }
        string requiredRoles = match.Endpoint.Roles;
        if (requiredRoles != null && requiredRoles != "") {
            try {
                IAuthorizationBehavior authorization =
                    (IAuthorizationBehavior)sp.GetService(typeof(IAuthorizationBehavior));
                Task authTask = authorization.AuthorizeAsync(context, requiredRoles);
                authTask.Wait();
            } catch (UnauthorizedException) {
                scope.Dispose();
                return new WebResponse(401, "Unauthorized", "{\"error\":\"unauthorized\"}");
            }
        }
        try {
            DispatchContext ctx = new DispatchContext();
            ctx.Sp = sp;
            ctx.BindJson = Binder.Build(req.Method, match, req.Body);
            object dispatched = match.Endpoint.Dispatcher.Dispatch(ctx);
            scope.Dispose();
            // 桥接：IWebResult 走 HTTP 契约响应；其余类型回退 JSON 序列化。
            if (dispatched is IWebResult) {
                return this.WebResponseFromResult((IWebResult)dispatched);
            }
            return new WebResponse(200, "OK", JsonSerializer.Serialize((IJsonSerializable)dispatched));
        } catch (Exception) {
            scope.Dispose();
            return new WebResponse(500, "Internal Server Error", "{\"error\":\"internal\"}");
        }
    }

    /// <summary>把 IWebResult 的 HTTP 契约转写为宿主 WebResponse（状态/原因/Content-Type/头/载荷）。</summary>
    private WebResponse WebResponseFromResult(IWebResult result) {
        WebResponse resp = new WebResponse(0, "", "");
        resp.Status = result.StatusCode;
        resp.Reason = this.ReasonPhrase(result.StatusCode);
        resp.ContentType = result.ContentType;
        resp.Headers = result.Headers;
        if (result.IsBinary) {
            resp.Data = result.Data;
            resp.IsBinary = true;
        } else {
            resp.Body = result.Body;
        }
        return resp;
    }

    /// <summary>状态码 → 标准原因短语（未知返回空）。</summary>
    private string ReasonPhrase(int status) {
        if (status == 200) { return "OK"; }
        if (status == 201) { return "Created"; }
        if (status == 204) { return "No Content"; }
        if (status == 302) { return "Found"; }
        if (status == 303) { return "See Other"; }
        if (status == 304) { return "Not Modified"; }
        if (status == 400) { return "Bad Request"; }
        if (status == 401) { return "Unauthorized"; }
        if (status == 403) { return "Forbidden"; }
        if (status == 404) { return "Not Found"; }
        if (status == 500) { return "Internal Server Error"; }
        return "";
    }
}

/// <summary>监听端点声明：host + 端口 + 协议 + TLS（对齐 Kestrel ListenOptions）。</summary>
public class ListenOptions {
    /// <summary>监听主机（"*"/"+" 为任意 IP；适配 Kestrel IPAddress）。诚实边界：当前经 Start(port) 绑定端口。</summary>
    public string Host;
    /// <summary>监听端口。</summary>
    public int Port;
    /// <summary>端点启用的 HTTP 协议（默认 Http1；对齐 Kestrel ListenOptions.Protocols）。</summary>
    public HttpProtocols Protocols;
    /// <summary>是否启用 TLS/HTTPS（对齐 Kestrel ListenOptions.UseHttps；诚实边界：暂未接入传输底座）。</summary>
    public bool Https;

    public ListenOptions(string host, int port, HttpProtocols protocols) {
        this.Host = host;
        this.Port = port;
        this.Protocols = protocols;
        this.Https = false;
    }

    /// <summary>启用 HTTPS（对齐 Kestrel ListenOptions.UseHttps()）。诚实边界：无 TLS 底座，
    /// 启动时如实报 https_not_implemented；本设置仅承载「声明意图」供配置回调表达。</summary>
    public void UseHttps() {
        this.Https = true;
    }
}

/// <summary>宿主内部响应载体：状态行 + 头 + Content-Type + 文本/二进制体（internal）。</summary>
internal class WebResponse {
    public int Status;
    public string Reason;
    public string ContentType;
    public WebHeaderCollection Headers;
    public bool IsBinary;
    public string Body;
    public byte[] Data;

    public WebResponse(int status, string reason, string body) {
        this.Status = status;
        this.Reason = reason;
        this.Body = body;
        this.ContentType = "application/json";
        this.Headers = new WebHeaderCollection();
        this.IsBinary = false;
        this.Data = new byte[0];
    }
}
