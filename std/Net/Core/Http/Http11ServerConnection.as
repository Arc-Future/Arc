// Arc.Net — HTTP/1.1 服务端连接原语。
//
// 服务端侧 HTTP/1.1 连接：包裹一条已接受（accept）的 TcpClient，负责
//   请求解析（请求行 + 头块 + Content-Length 体，RFC 7230 §3 分帧）与
//   响应写出（状态行 + 头 + Content-Length 体）。
// 供 Arc.Web（WebApplication 宿主）作传输底座消费；纯 Arc 实现，复用
// NetworkStream（ReadLine/ReadString）与 WebHeaderCollection 既有原语。
//
// 诚实边界（对齐 Arc.Net 既有服务端原语 Http2ServerConnection）：
//   - 单连接顺序处理（一次一个请求）；请求体按 Content-Length 累积（chunked 请求体后置）。
//   - 响应统一 Content-Length（无 chunked 响应流式；流式面后置）。
//   - obs-fold 头折叠按 RFC 7230 §3.2.4 合并。
//   - 体分帧按字节计数：Content-Length 体经真字节读读满 n 字节后整体 UTF-8 解码为
//     string（Body 为 string，Arc C-string 模型）——载荷含内部 0x00 的请求体在 string
//     边界截断，二进制体全保真对传（BodyBytes 字节面）后置。
namespace Arc.Net;

using Arc;
using Arc.Text;

/// <summary>
/// HTTP/1.1 服务端连接：读完整请求（行 + 头 + 体）与写完整响应。
/// 同步传输原语（ReadRequest/WriteResponse）+ Reactor 真异步面
/// （ReadRequestAsync/WriteResponseAsync，RFC 038 M2——请求解析经
/// NetworkStream.ReadLineAsync/ReadBytesAsync，响应写出经 WriteStringAsync/
/// WriteBytesAsync，不阻塞调用线程）；供服务端宿主在异步 accept 后逐连接消费。
/// </summary>
public class Http11ServerConnection {
    private NetworkStream _stream;

    public Http11ServerConnection(TcpClient client, int timeout) {
        _stream = new NetworkStream(client, timeout);
    }

    /// <summary>读完整一个请求（请求行 + 头 + Content-Length 体）。EOF/解析失败返回 null。</summary>
    public HttpServerRequest ReadRequest() {
        string requestLine = _stream.ReadLine();
        if (requestLine == null || requestLine == "") { return null; }

        int sp1 = requestLine.IndexOf(" ");
        if (sp1 <= 0) { return null; }
        string method = requestLine.Substring(0, sp1);
        string rest = requestLine.Substring(sp1 + 1, requestLine.Length - sp1 - 1);
        int sp2 = rest.IndexOf(" ");
        string path = sp2 > 0 ? rest.Substring(0, sp2) : rest;

        WebHeaderCollection headers = new WebHeaderCollection();
        string lastHeaderName = "";
        string lastHeaderValue = "";
        while (true) {
            string hl = _stream.ReadLine();
            if (hl == null) { return null; }
            if (hl == "") { break; }
            // obs-fold（RFC 7230 §3.2.4）：SP/HTAB 开头的续行折叠并入上一个头。
            if (hl.Length > 0 && (hl.Substring(0, 1) == " " || hl.Substring(0, 1) == "\t")) {
                if (lastHeaderName != "") {
                    lastHeaderValue = lastHeaderValue + " " + hl.Trim();
                    headers.Remove(lastHeaderName);
                    headers.Add(lastHeaderName, lastHeaderValue);
                }
                continue;
            }
            int cp = hl.IndexOf(":");
            if (cp > 0) {
                string name = hl.Substring(0, cp).Trim();
                string value = hl.Substring(cp + 1, hl.Length - cp - 1).Trim();
                headers.Add(name, value);
                lastHeaderName = name;
                lastHeaderValue = value;
            }
        }

        string body = "";
        int cl = headers.ContentLength();
        if (cl > 0) {
            body = this.ReadN(cl);
        }

        HttpServerRequest req = new HttpServerRequest();
        req.Method = method;
        req.Path = path;
        req.Headers = headers;
        req.Body = body;
        return req;
    }

    /// <summary>
    /// 异步读完整一个请求（请求行 + 头 + Content-Length 体）。EOF/解析失败返回 null。
    /// 基于 <see cref="NetworkStream.ReadLineAsync"/> / <see cref="NetworkStream.ReadBytesAsync"/>
    /// （Reactor 真异步，RFC 038 M2），不阻塞调用线程——服务端连接处理全异步化。
    /// </summary>
    public async Task<HttpServerRequest> ReadRequestAsync() {
        string requestLine = await _stream.ReadLineAsync();
        if (requestLine == null || requestLine == "") { return null; }

        int sp1 = requestLine.IndexOf(" ");
        if (sp1 <= 0) { return null; }
        string method = requestLine.Substring(0, sp1);
        string rest = requestLine.Substring(sp1 + 1, requestLine.Length - sp1 - 1);
        int sp2 = rest.IndexOf(" ");
        string path = sp2 > 0 ? rest.Substring(0, sp2) : rest;

        WebHeaderCollection headers = new WebHeaderCollection();
        string lastHeaderName = "";
        string lastHeaderValue = "";
        while (true) {
            string hl = await _stream.ReadLineAsync();
            if (hl == null) { return null; }
            if (hl == "") { break; }
            // obs-fold（RFC 7230 §3.2.4）：SP/HTAB 开头的续行折叠并入上一个头。
            if (hl.Length > 0 && (hl.Substring(0, 1) == " " || hl.Substring(0, 1) == "\t")) {
                if (lastHeaderName != "") {
                    lastHeaderValue = lastHeaderValue + " " + hl.Trim();
                    headers.Remove(lastHeaderName);
                    headers.Add(lastHeaderName, lastHeaderValue);
                }
                continue;
            }
            int cp = hl.IndexOf(":");
            if (cp > 0) {
                string name = hl.Substring(0, cp).Trim();
                string value = hl.Substring(cp + 1, hl.Length - cp - 1).Trim();
                headers.Add(name, value);
                lastHeaderName = name;
                lastHeaderValue = value;
            }
        }

        string body = "";
        int cl = headers.ContentLength();
        if (cl > 0) {
            body = await this.ReadNAsync(cl);
        }

        HttpServerRequest req = new HttpServerRequest();
        req.Method = method;
        req.Path = path;
        req.Headers = headers;
        req.Body = body;
        return req;
    }

    /// <summary>
    /// 写完整响应（状态行 + 头 + Content-Type + Content-Length 体）。
    /// <paramref name="data"/> 非空二进制时以二进制载荷写出，否则写文本 <paramref name="body"/>；
    /// <paramref name="contentType"/> 为空回退 application/json。返回是否写出成功。
    /// </summary>
    public bool WriteResponse(int status, string reason, WebHeaderCollection headers, string contentType, string body, byte[] data) {
        bool isBinary = data != null && data.Length > 0;
        string resp = "HTTP/1.1 " + Convert.ToString(status) + " " + reason + "\r\n";
        if (headers != null) {
            string hs = headers.ToHeaderString();
            if (hs != "") {
                resp = resp + hs + "\r\n";
            }
        }
        int length = isBinary ? data.Length : (body != null ? body.Length : 0);
        string ct = (contentType != null && contentType != "") ? contentType : "application/json";
        resp = resp + "Content-Length: " + Convert.ToString(length) + "\r\n";
        resp = resp + "Content-Type: " + ct + "\r\n";
        resp = resp + "\r\n";
        if (isBinary) {
            int sent = _stream.WriteString(resp);
            _stream.Write(data, 0, data.Length);
            return sent > 0;
        }
        if (body != null && body != "") { resp = resp + body; }
        int written = _stream.WriteString(resp);
        return written > 0;
    }

    /// <summary>
    /// 异步写完整响应（状态行 + 头 + Content-Type + Content-Length 体）。
    /// <paramref name="data"/> 非空二进制时以二进制载荷写出，否则写文本 <paramref name="body"/>；
    /// <paramref name="contentType"/> 为空回退 application/json。返回是否写出成功。
    /// 基于 <see cref="NetworkStream.WriteStringAsync"/> / <see cref="NetworkStream.WriteBytesAsync"/>
    /// （Reactor 真异步，RFC 038 M2），不阻塞调用线程。
    /// </summary>
    public async Task<bool> WriteResponseAsync(int status, string reason, WebHeaderCollection headers, string contentType, string body, byte[] data) {
        bool isBinary = data != null && data.Length > 0;
        string resp = "HTTP/1.1 " + Convert.ToString(status) + " " + reason + "\r\n";
        if (headers != null) {
            string hs = headers.ToHeaderString();
            if (hs != "") {
                resp = resp + hs + "\r\n";
            }
        }
        int length = isBinary ? data.Length : (body != null ? body.Length : 0);
        string ct = (contentType != null && contentType != "") ? contentType : "application/json";
        resp = resp + "Content-Length: " + Convert.ToString(length) + "\r\n";
        resp = resp + "Content-Type: " + ct + "\r\n";
        resp = resp + "\r\n";
        if (isBinary) {
            int sent = await _stream.WriteStringAsync(resp);
            if (sent <= 0) { return false; }
            await _stream.WriteBytesAsync(data, 0, data.Length);
            return true;
        }
        if (body != null && body != "") { resp = resp + body; }
        int written = await _stream.WriteStringAsync(resp);
        return written > 0;
    }

    /// <summary>关闭底层连接。</summary>
    public void Close() {
        _stream.Close();
    }

    private string ReadN(int n) {
        byte[] buf = new byte[n];
        int got = 0;
        while (got < n) {
            int k = _stream.Read(buf, got, n - got);
            if (k <= 0) { break; }
            got = got + k;
        }
        if (got == n) {
            return Encoding.GetString(buf);
        }
        byte[] part = new byte[got];
        Array.Copy(buf, 0, part, 0, got);
        return Encoding.GetString(part);
    }

    /// <summary>异步累积读取恰好 n 字节（部分读语义循环）后整体 UTF-8 解码。
    /// 基于 ReadBytesAsync（Reactor 真异步字节读，无 NUL 截断）。</summary>
    private async Task<string> ReadNAsync(int n) {
        byte[] buf = new byte[n];
        int got = 0;
        while (got < n) {
            int k = await _stream.ReadBytesAsync(buf, got, n - got);
            if (k <= 0) { break; }
            got = got + k;
        }
        if (got == n) {
            return Encoding.GetString(buf);
        }
        byte[] part = new byte[got];
        Array.Copy(buf, 0, part, 0, got);
        return Encoding.GetString(part);
    }
}
