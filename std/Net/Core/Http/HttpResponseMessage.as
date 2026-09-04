// RFC 025 M4: Arc.Net — HTTP 响应消息。
//
// 对标 C# System.Net.Http.HttpResponseMessage（.NET 9）。
// 封装完整的 HTTP 响应：状态码 + 响应头 + 响应体。

namespace Arc.Net;

/// <summary>
/// HTTP 响应消息——包含状态码、响应头和响应体。
///
/// 由 HttpClient.Get / Post 等方法返回。
/// 使用 using 或 Dispose 确保底层连接释放。
/// </summary>
public class HttpResponseMessage : IDisposable {
    /// <summary>HTTP 状态码（如 200, 404, 500）。</summary>
    public int StatusCode;

    /// <summary>HTTP 版本（如 "HTTP/1.1"）。</summary>
    public string Version;

    /// <summary>HTTP 原因短语（如 "OK", "Not Found"）。</summary>
    public string ReasonPhrase;

    /// <summary>响应头集合。</summary>
    public WebHeaderCollection Headers;

    /// <summary>chunked 响应的 trailer 头（RFC 7230 §4.1.2）；非 chunked 为空集合。</summary>
    public WebHeaderCollection Trailers;

    /// <summary>响应体原始字符串。</summary>
    public string Body;

    /// <summary>响应内容（HttpContent 表示；由解析层填充 Body + Content-Type）。</summary>
    public HttpContent Content;

    /// <summary>主机名。</summary>
    public string _host;

    /// <summary>端口号。</summary>
    public int _port;

    /// <summary>底层 TCP 连接（用于 Keep-Alive 复用）。</summary>
    public TcpClient _connection;

    /// <summary>连接是否可复用（响应体无明确分帧读到 EOF 时置 false）。</summary>
    public bool _keepAlive;

    /// <summary>流式响应的活动传输载体（未消费体；null = 已全缓冲）。</summary>
    public StreamTransport _live;

    /// <summary>流式响应是否 chunked 分帧（经 _live 增量读时以 ChunkedStreamReader 解码）。</summary>
    public bool _chunked;

    /// <summary>创建空的响应消息。</summary>
    public HttpResponseMessage() {
        this.StatusCode = 0;
        this.Version = "HTTP/1.1";
        this.ReasonPhrase = "";
        this.Headers = new WebHeaderCollection();
        this.Trailers = new WebHeaderCollection();
        this.Body = "";
        this.Content = null;
        _host = "";
        _port = 0;
        _keepAlive = true;
        _live = null;
        _chunked = false;
    }

    /// <summary>判断状态码是否表示成功（2xx）。</summary>
    public bool IsSuccessStatusCode() {
        return this.StatusCode >= 200 && this.StatusCode < 300;
    }
    public bool EnsureSuccessStatusCode() {
        if (!this.IsSuccessStatusCode()) {
            return false;
        }
        return true;
    }

    /// <summary>流式响应的活动传输载体；全缓冲响应返回 null。调用方以
    /// ChunkedStreamReader.ReadChunk()（chunked）或 ReadString() 增量读。</summary>
    public StreamTransport LiveStream {
        get { return _live; }
    }

    /// <summary>流式响应是否 chunked 分帧。</summary>
    public bool IsChunkedStreaming {
        get { return _chunked; }
    }

    /// <summary>关闭底层 TCP 连接。</summary>
    public void Close() {
        if (_connection != null) {
            _connection.Close();
            _connection = null;
        }
    }

    /// <summary>释放 HTTP 响应消息的资源。</summary>
    public void Dispose() {
        this.Close();
    }
}
