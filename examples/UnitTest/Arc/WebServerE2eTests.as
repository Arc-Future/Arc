namespace UnitTest.Arc;

using Arc;
using Arc.Net;
using Arc.QIF;
using Arc.Text.Json;
using Arc.Web;

/// <summary>环回 e2e 请求契约：GET/POST 端点绑定载体。</summary>
public class PingRequest : IJsonDeserializable, IRequest<PingResponse>
{
    public string Name;
    public void ReadJson(JsonReader reader)
    {
        while (reader.Read())
        {
            if (reader.TokenType == JsonTokenType.EndObject) { return; }
            if (reader.TokenType == JsonTokenType.PropertyName)
            {
                string prop = reader.GetString();
                reader.Read();
                if (prop == "name") { Name = reader.GetString(); }
                else { reader.Skip(); }
            }
        }
    }
}

/// <summary>环回 e2e 响应契约：JSON 序列化载体。</summary>
public class PingResponse : IJsonSerializable
{
    public string Reply;
    public void WriteJson(JsonWriter writer)
    {
        writer.WriteStartObject();
        writer.WriteString("reply", Reply);
        writer.WriteEndObject();
    }
}

/// <summary>环回 e2e 处理器：echo 回复。</summary>
public class PingHandler : IRequestHandler<PingRequest, PingResponse>
{
    public Task<PingResponse> HandleAsync(PingRequest request, CancellationToken cancellationToken)
    {
        PingResponse r = new PingResponse();
        r.Reply = "pong:" + (request.Name == null ? "" : request.Name);
        return Task.FromResult(r);
    }
}

/// <summary>
/// Web 服务端 HTTP/1.1 环回 e2e（RFC 038 M2 / P0-b）：验证 WebApplication 异步
/// accept 循环 + Http11ServerConnection 异步读请求/写响应全链 Reactor 真异步——
/// 移除阻塞读占线程池后，客户端经 HttpClient 请求可完整往返。
/// 判别信号：真实 loopback TCP 上客户端请求 → 服务端 async accept →
/// async 读请求 → 分发 → async 写响应 → 客户端读到响应体。
/// </summary>
public class WebServerE2eTests
{
    // ── 全栈 e2e：WebApplication 异步 accept + HttpClient 往返 ──

    [Fact]
    public async Task Web_AsyncAccept_HttpClient_E2e()
    {
        int port = 29123;
        WebApplication app = new WebApplication();
        app.AddServices(sc => sc.AddTransient<IRequestHandler<PingRequest, PingResponse>, PingHandler>());
        app.MapPost<PingRequest, PingResponse>("/api/ping");
        app.ListenLocalhost(port);
        Task run = app.RunAsync(); // fire-and-forget：异步 accept 循环常驻（测试进程结束后终止）

        // 有界轮询等待监听就绪（async accept 启动与绑定存在竞态窗口）。
        bool ready = false;
        for (int i = 0; i < 100; i++)
        {
            TcpClient probe = new TcpClient();
            if (probe.Connect("127.0.0.1", port))
            {
                probe.Close();
                ready = true;
                break;
            }
            await Task.Delay(50);
        }
        Assert.True(ready, "web_server_not_ready");

        HttpClient http = new HttpClient();
        HttpResponseMessage resp = await http.PostAsync(
            "http://127.0.0.1:" + port + "/api/ping",
            "{\"name\":\"arc\"}");
        Assert.True(resp != null, "web_response_null");
        string body = resp.Body;
        Assert.True(body.IndexOf("pong:arc") >= 0, "web_echo_mismatch body=" + body);
        http.Dispose();
    }

    // ── 传输级 e2e：Http11ServerConnection 异步读/写（GET）──

    [Fact]
    public async Task Web_Http11_AsyncReadWrite_RawSocket()
    {
        int port = 29124;
        TcpListener listener = new TcpListener();
        Assert.True(listener.Start(port), "listener_bind_failed");

        TcpClient client = new TcpClient();
        Assert.True(client.Connect("127.0.0.1", port), "client_connect_failed");
        TcpClient serverClient = await listener.AcceptTcpClientAsync();
        Assert.NotNull(serverClient);

        // 客户端发原始 HTTP/1.1 GET → 服务端异步读请求。
        client.Send("GET /hello HTTP/1.1\r\nHost: localhost\r\nX-Test: 1\r\n\r\n");
        Http11ServerConnection conn = new Http11ServerConnection(serverClient, 30000);
        HttpServerRequest req = await conn.ReadRequestAsync();
        Assert.NotNull(req);
        Assert.Equal("GET", req.Method);
        Assert.Equal("/hello", req.Path);

        // 服务端异步写响应 → 客户端读到响应。
        WebHeaderCollection headers = new WebHeaderCollection();
        bool wrote = await conn.WriteResponseAsync(200, "OK", headers, "text/plain", "pong", null);
        Assert.True(wrote);
        string response = client.Receive();
        Assert.True(response.IndexOf("200 OK") >= 0, "web_status_missing response=" + response);
        Assert.True(response.IndexOf("pong") >= 0, "web_body_missing response=" + response);

        conn.Close();
        client.Close();
        listener.Stop();
    }
}
