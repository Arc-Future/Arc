// RFC 033 §1.0: Arc.Net — HTTP/1.1 连接（对齐 C# Http11Connection / HttpConnectionBase）。
//
// 单一职责：包裹一条传输载体（StreamTransport——明文 NetworkStream 或 TLS
// TlsNetworkStream），负责 HTTP/1.1 请求序列化（BuildRequest）与响应解析
// （ParseResponse，RFC 7230 §3.3.3 分帧）。连接池归属 HttpConnectionPool，
// 门面归属 HttpClient/handler。
//
// 传输抽象化：自 StreamTransport 契约（RFC 025 M4）固化后，本层不再直接依赖
// NetworkStream，而是面向 StreamTransport——https 直连经 TlsNetworkStream 透明
// 复用同一套序列化/解析逻辑而不改写（RFC 033 S3 字节层桥接、RFC 026 深层 https）。
//
// 流式响应面：`ParseStreaming` 解析状态行/头并确定分帧，但**不消费响应体**——把
// 活动 StreamTransport 交给调用方（HttpResponseMessage.LiveStream），由消费者以
// ChunkedStreamReader.ReadChunk()/ReadString() 增量读（SSE 逐块消费；对齐业界
// HttpContent.ReadAsStream 暴露活传输单一惯用法）。`Parse` 保持全缓冲向后兼容。
namespace Arc.Net;

/// <summary>
/// HTTP/1.1 连接——单条传输载体上的请求序列化与响应解析。
/// 对齐 C# Http11Connection。支持 chunked（含 chunk 扩展 / trailer）、
/// Content-Length、keep-alive 分帧；无明确分帧读到 EOF 标记不可复用。
/// </summary>
public class Http11Connection {
    private StreamTransport _transport;
    private TcpClient _tcp;
    private int _timeout;

    /// <summary>以显式传输载体构造（https 时 transport = TlsNetworkStream；http 时 = NetworkStream）。</summary>
    public Http11Connection(StreamTransport transport, TcpClient tcp, int timeout) {
        _transport = transport;
        _tcp = tcp;
        _timeout = timeout;
    }

    /// <summary>以明文 TCP 构造（向后兼容：自动包装 NetworkStream）。</summary>
    public Http11Connection(TcpClient tcp, int timeout) {
        _tcp = tcp;
        _timeout = timeout;
        _transport = new NetworkStream(tcp, timeout);
    }

    /// <summary>底层是否已连接。</summary>
    public bool Connected {
        get { return _tcp != null && _tcp.Connected; }
    }

    /// <summary>底层 TcpClient（供连接池/门面复用或关闭）。</summary>
    public TcpClient Tcp {
        get { return _tcp; }
    }

    /// <summary>当前传输载体（明文 NetworkStream 或 TLS TlsNetworkStream）。</summary>
    public StreamTransport Transport {
        get { return _transport; }
    }

    /// <summary>序列化并发送完整请求（请求行 + 头 + 体）。返回是否连接仍有效。</summary>
    public bool Send(string request) {
        if (_transport == null) { return false; }
        _transport.WriteString(request);
        return this.Connected;
    }

    /// <summary>异步序列化并发送完整请求（真异步 · Reactor 提交 write）。返回是否连接仍有效。</summary>
    public async Task<bool> SendAsync(string request) {
        if (_transport == null) { return false; }
        await _transport.WriteStringAsync(request);
        return this.Connected;
    }

    /// <summary>读取并解析完整响应（全缓冲；对齐 RFC 7230 §3.3.3 分帧）。失败返回 null。</summary>
    public HttpResponseMessage Parse(string method) {
        return this.ParseResponse(method, false);
    }

    /// <summary>异步读取并解析完整响应（全缓冲；真异步）。失败返回 null。</summary>
    public Task<HttpResponseMessage> ParseAsync(string method) {
        return this.ParseResponseAsync(method, false);
    }

    /// <summary>
    /// 读取并解析响应头 + 分帧元数据，**不消费响应体**（流式）。
    /// 返回的响应经 <c>LiveStream</c> 暴露活动 StreamTransport，调用方以
    /// ChunkedStreamReader.ReadChunk()（chunked）或 ReadString() 增量读。
    /// </summary>
    public HttpResponseMessage ParseStreaming(string method) {
        return this.ParseResponse(method, true);
    }

    /// <summary>
    /// 异步读取并解析响应头 + 分帧元数据，**不消费响应体**（流式；真异步）。
    /// 返回的响应经 <c>LiveStream</c> 暴露活动 StreamTransport，调用方以
    /// ChunkedStreamReader（chunked）或 async ReadString 增量读。
    /// </summary>
    public Task<HttpResponseMessage> ParseStreamingAsync(string method) {
        return this.ParseResponseAsync(method, true);
    }

    /// <summary>关闭底层连接与传输。</summary>
    public void Close() {
        if (_transport != null) {
            _transport.Close();
            _transport = null;
        }
        if (_tcp != null) {
            _tcp.Close();
            _tcp = null;
        }
    }

    // ── 请求构建（静态；供 HttpClient 与 SocketsHttpHandler 共用）──

    /// <summary>构建 HTTP/1.1 请求行 + 头 + 体（RFC 7230 §6.3 keep-alive）。</summary>
    public static string BuildRequest(string method, string path, string host, string body, string ct) {
        string r = method + " " + path + " HTTP/1.1\r\nHost: " + host + "\r\nConnection: keep-alive\r\n";
        if (body != "") {
            if (ct != "") { r = r + "Content-Type: " + ct + "\r\n"; }
            r = r + "Content-Length: " + Convert.ToString(body.Length) + "\r\n";
        }
        r = r + "\r\n";
        if (body != "") { r = r + body; }
        return r;
    }

    // ── 响应解析 ──

    /// <summary>
    /// 解析 HTTP 响应（RFC 7230 §3.3.3 分帧）：
    ///   状态行（版本 + 状态码 + 原因短语）；头（大小写不敏感、obs-fold 折叠）；
    ///   HEAD/1xx/204/304 无体；chunked → ChunkedStreamReader；Content-Length → 精确读；
    ///   两者皆无 → 读到 EOF 并标记连接不可复用。
    /// <paramref name="streaming"/> 为 true 时解析头后不消费体，把活动传输交给调用方。
    /// </summary>
    private HttpResponseMessage ParseResponse(string method, bool streaming) {
        var r = new HttpResponseMessage();
        string sl = _transport.ReadLine(); if (sl == null || sl == "") { return null; }
        this.ParseStatus(sl, r);
        r.Headers = new WebHeaderCollection();
        string lastHeaderName = "";
        string lastHeaderValue = "";
        while (true) {
            string hl = _transport.ReadLine();
            if (hl == null) { return null; }
            if (hl == "") { break; }
            // obs-fold（RFC 7230 §3.2.4）：以 SP/HTAB 开头的续行折叠并入上一个头。
            if (hl.Length > 0 && (hl.Substring(0, 1) == " " || hl.Substring(0, 1) == "\t")) {
                if (lastHeaderName != "") {
                    lastHeaderValue = lastHeaderValue + " " + hl.Trim();
                    r.Headers.Remove(lastHeaderName);
                    r.Headers.Add(lastHeaderName, lastHeaderValue);
                }
                continue;
            }
            int cp = hl.IndexOf(":");
            if (cp > 0) {
                string name = hl.Substring(0, cp).Trim();
                string value = hl.Substring(cp + 1, hl.Length - cp - 1).Trim();
                r.Headers.Add(name, value);
                lastHeaderName = name;
                lastHeaderValue = value;
            }
        }
        // 响应体分帧（RFC 7230 §3.3.3）。
        bool noBody = method == "HEAD"
            || (r.StatusCode >= 100 && r.StatusCode < 200)
            || r.StatusCode == 204 || r.StatusCode == 304;
        string te = r.Headers.Get("Transfer-Encoding");
        bool chunked = te != "" && te.ToLower() == "chunked";
        if (noBody) {
            r.Body = "";
        } else if (streaming) {
            // 流式：不消费体，暴露活动传输 + 分帧标记，交调用方增量读。
            r._live = _transport;
            r._chunked = chunked;
            r._keepAlive = false; // 体未排水，连接不复用
            r.Body = "";
        } else if (chunked) {
            var cr = new ChunkedStreamReader(_transport);
            r.Body = cr.ReadAllChunks();
            r.Trailers = cr.Trailers;
        } else {
            int cl = r.Headers.ContentLength();
            if (cl >= 0) {
                r.Body = this.ReadN(_transport, cl);
            } else {
                // 无明确分帧 → 读到 EOF；连接不可复用。
                r.Body = _transport.ReadToEnd();
                r._keepAlive = false;
            }
        }
        // 填充 HttpContent（Body + Content-Type），供统一读取面使用。
        if (streaming && !noBody) {
            // 流式：Content 以活传输承载（ReadAsStream 返回活传输）。
            r.Content = new StreamContent("");
            r.Content.ContentType = r.Headers.Get("Content-Type");
            r.Content.LiveTransport = _transport;
            r._chunked = chunked;
        } else {
            r.Content = new StringContent(r.Body, r.Headers.Get("Content-Type"));
        }
        return r;
    }

    private string ReadN(StreamTransport ns, int n) {
        string r = "";
        while (r.Length < n) {
            string c = ns.ReadString(n - r.Length);
            if (c == null || c == "") { break; }
            r = r + c;
        }
        return r;
    }

    /// <summary>异步版 <see cref="ParseResponse(string, bool)"/>——以真异步传输方法面
    /// （ReadLineAsync/ReadStringAsync/ReadToEndAsync）增量读，不阻塞调用线程。</summary>
    private async Task<HttpResponseMessage> ParseResponseAsync(string method, bool streaming) {
        var r = new HttpResponseMessage();
        string sl = await _transport.ReadLineAsync(); if (sl == null || sl == "") { return null; }
        this.ParseStatus(sl, r);
        r.Headers = new WebHeaderCollection();
        string lastHeaderName = "";
        string lastHeaderValue = "";
        while (true) {
            string hl = await _transport.ReadLineAsync();
            if (hl == null) { return null; }
            if (hl == "") { break; }
            if (hl.Length > 0 && (hl.Substring(0, 1) == " " || hl.Substring(0, 1) == "\t")) {
                if (lastHeaderName != "") {
                    lastHeaderValue = lastHeaderValue + " " + hl.Trim();
                    r.Headers.Remove(lastHeaderName);
                    r.Headers.Add(lastHeaderName, lastHeaderValue);
                }
                continue;
            }
            int cp = hl.IndexOf(":");
            if (cp > 0) {
                string name = hl.Substring(0, cp).Trim();
                string value = hl.Substring(cp + 1, hl.Length - cp - 1).Trim();
                r.Headers.Add(name, value);
                lastHeaderName = name;
                lastHeaderValue = value;
            }
        }
        bool noBody = method == "HEAD"
            || (r.StatusCode >= 100 && r.StatusCode < 200)
            || r.StatusCode == 204 || r.StatusCode == 304;
        string te = r.Headers.Get("Transfer-Encoding");
        bool chunked = te != "" && te.ToLower() == "chunked";
        if (noBody) {
            r.Body = "";
        } else if (streaming) {
            r._live = _transport;
            r._chunked = chunked;
            r._keepAlive = false;
            r.Body = "";
        } else if (chunked) {
            var cr = new ChunkedStreamReader(_transport);
            r.Body = cr.ReadAllChunks();
            r.Trailers = cr.Trailers;
        } else {
            int cl = r.Headers.ContentLength();
            if (cl >= 0) {
                r.Body = await this.ReadNAsync(_transport, cl);
            } else {
                r.Body = await _transport.ReadToEndAsync();
                r._keepAlive = false;
            }
        }
        if (streaming && !noBody) {
            r.Content = new StreamContent("");
            r.Content.ContentType = r.Headers.Get("Content-Type");
            r.Content.LiveTransport = _transport;
            r._chunked = chunked;
        } else {
            r.Content = new StringContent(r.Body, r.Headers.Get("Content-Type"));
        }
        return r;
    }

    private async Task<string> ReadNAsync(StreamTransport ns, int n) {
        string r = "";
        while (r.Length < n) {
            string c = await ns.ReadStringAsync(n - r.Length);
            if (c == null || c == "") { break; }
            r = r + c;
        }
        return r;
    }

    private void ParseStatus(string line, HttpResponseMessage r) {
        int s1 = line.IndexOf(" "); if (s1 <= 0) { return; }
        r.Version = line.Substring(0, s1);
        string a = line.Substring(s1 + 1, line.Length - s1 - 1);
        int s2 = a.IndexOf(" ");
        r.StatusCode = s2 > 0 ? this.PI(a.Substring(0, s2)) : this.PI(a);
        if (s2 > 0) { r.ReasonPhrase = a.Substring(s2 + 1, a.Length - s2 - 1); }
    }

    private int PI(string s) {
        try {
            return Convert.ToInt32(s, 10);
        } catch {
            return 0;
        }
    }
}
