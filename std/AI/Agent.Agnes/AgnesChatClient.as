// RFC 038：Agnes chat client —— 统一走 Arc.Net.HttpClient（复用连接池）。
//
// 门面：只编排「建请求 → 发 HTTP → 交给 parser」。请求体构造（AgnesRequestBuilder）、
// 非流式响应解析（AgnesResponseParser）、流式事件序列（AgnesEventStream + 通用
// SseDecoder 领域映射）各自独立模块。
// Implements IAIChatClient through AIChatClient base class.
namespace Arc.Agent.Agnes;
using Arc;
using Arc.Agent;
using Arc.Collections;
using Arc.Net;

public class AgnesChatClient : AIChatClient, IDisposable
{
    private AgnesOptions _options;
    private HttpClient _http;
    private bool _disposed;

    /// <summary>以配置构造；默认 <see cref="HttpClient"/>（复用连接池）。</summary>
    public AgnesChatClient(AgnesOptions options)
    {
        this.Init(options, null);
    }

    /// <summary>以配置 + 显式 handler 构造（测试回放注入 ReplayHttpHandler；null → 默认 HttpClient）。</summary>
    public AgnesChatClient(AgnesOptions options, HttpMessageHandler handler)
    {
        this.Init(options, handler != null ? new HttpClient(handler) : null);
    }

    private void Init(AgnesOptions options, HttpClient http)
    {
        if (options == null)
        {
            throw new ArgumentNullException("options");
        }
        _options = options;
        _http = http != null ? http : new HttpClient();
        _disposed = false;
    }

    /// <summary>释放内部 <see cref="HttpClient"/>（幂等；null 安全）。</summary>
    public void Dispose()
    {
        if (_disposed)
        {
            return;
        }
        _disposed = true;
        if (_http != null)
        {
            _http.Dispose();
            _http = null;
        }
    }

    public async Task<AIReply> CompleteAsync(AIRequest request, CancellationToken cancellationToken)
    {
        if (cancellationToken.IsCancellationRequested)
        {
            return AIReply.Fail("Cancelled", "AgnesChatClient.CompleteAsync: canceled before start");
        }
        if (request == null)
        {
            return AIReply.Fail("ArgumentNull", "request is null");
        }

        string body = AgnesRequestBuilder.BuildRequestJson(request, _options, false);

        HttpRequestMessage req = new HttpRequestMessage(HttpMethod.POST, new Uri(this.ResolveUrl()));
        req.Headers.Add("Authorization", "Bearer " + _options.ApiKey);
        req.Content = new StringContent(body, "application/json");

        HttpResponseMessage resp = await _http.SendAsync(req);
        if (resp == null)
        {
            return AIReply.Fail("NetworkError", "Failed to connect to " + _options.BaseUrl);
        }

        string respBody = resp.Body != null ? resp.Body : "";
        int statusCode = resp.StatusCode;
        resp.Dispose();

        if (statusCode != 200)
        {
            return AIReply.Fail("HttpError", AgnesResponseParser.ExtractErrorMsg(respBody, statusCode));
        }

        return AgnesResponseParser.ParseNonStreamResponse(respBody);
    }

    /// <summary>
    /// 流式事件序列（IAsyncEnumerable 单一惯用法，原生 yield 迭代器）：HTTP 发送
    /// 延迟到首次枚举（冷流），逐 SSE 事件映射为 AIStreamEvent；流恰以
    /// Completed/Error 终结事件收尾。取消/空请求校验失败以冷错误流返回。
    /// </summary>
    public IAsyncEnumerable<AIStreamEvent> StreamEventsAsync(AIRequest request, CancellationToken cancellationToken)
    {
        if (cancellationToken.IsCancellationRequested)
        {
            return AIStreamEvent.ErrorStream("Cancelled", "AgnesChatClient.StreamEventsAsync: canceled before start");
        }
        if (request == null)
        {
            return AIStreamEvent.ErrorStream("ArgumentNull", "request is null");
        }
        return AgnesEventStream.Events(_options, _http, this.ResolveUrl(), request, cancellationToken);
    }

    /// <summary>把 BaseUrl 归一为完整请求端点：去尾部斜杠后补 "/chat/completions"。
    /// 官方 Base URL 已含 "/v1" 路径，端点即 ".../v1/chat/completions"。</summary>
    private string ResolveUrl()
    {
        string url = _options.BaseUrl != null && _options.BaseUrl != "" ? _options.BaseUrl : "https://apihub.agnes-ai.com/v1";
        if (url.Length > 0 && url.Substring(url.Length - 1, 1) == "/")
        {
            url = url.Substring(0, url.Length - 1);
        }
        return url + "/chat/completions";
    }
}
