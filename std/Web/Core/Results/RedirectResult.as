// RedirectResult —— IRedirectResult 默认实现（RFC 040 §5）：PRG 重定向结果。
// 实现 IWebResult 统一 HTTP 契约：302 + Location 头（经 Headers 携带，HTTP/1.1 直接写、
// HTTP/2 经 Get("Location") 回带），无载荷。契约成员使用 getter-only auto-property
// （构造期赋值/初值），消除显式 getter + backing field 冗余；getter 零成本读字段。
namespace Arc.Web;

using Arc.Net;

/// <summary>重定向结果实现：Handler 经 PageHandler.Redirect(url) 返回。</summary>
public class RedirectResult : IRedirectResult {
    /// <summary>重定向目标 URL。</summary>
    public string Url { get; }

    /// <summary>响应头集合（含 Location）。</summary>
    public WebHeaderCollection Headers { get; }

    /// <summary>HTTP 状态码（302 Found）。</summary>
    public int StatusCode { get; } = 302;

    /// <summary>无响应体，Content-Type 为空。</summary>
    public string ContentType { get; } = "";

    /// <summary>文本载荷（无）。</summary>
    public bool IsBinary { get; }

    /// <summary>文本响应体（重定向无体）。</summary>
    public string Body { get; } = "";

    /// <summary>二进制载荷（恒空）。</summary>
    public byte[] Data { get; } = new byte[0];

    public RedirectResult(string url) {
        this.Url = url;
        this.Headers = new WebHeaderCollection();
        this.Headers.Add("Location", url);
    }
}