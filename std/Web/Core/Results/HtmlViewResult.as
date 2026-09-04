// HtmlViewResult —— IHtmlView 默认实现（RFC 040 §5）：编译器 SSR 渲染代码的载体。
namespace Arc.Web;

using Arc.Net;

/// <summary>HTML 视图结果实现：编译器将 PageHandler.View(model) 改写为 new HtmlViewResult(渲染函数(model))。
/// HTTP 契约：200 + text/html; charset=utf-8 + 文本载荷。</summary>
public class HtmlViewResult : IHtmlView {
    /// <summary>渲染完成的 HTML 文档字符串。</summary>
    public string Html { get; }

    /// <summary>HTTP 状态码（200 OK）。</summary>
    public int StatusCode { get; } = 200;

    /// <summary>Content-Type：text/html; charset=utf-8。</summary>
    public string ContentType { get; } = "text/html; charset=utf-8";

    /// <summary>响应头集合（可扩展缓存/会话等）。</summary>
    public WebHeaderCollection Headers { get; }

    /// <summary>文本载荷（非二进制）。</summary>
    public bool IsBinary { get; }

    /// <summary>文本响应体 = 渲染后的 HTML。</summary>
    public string Body { get { return this.Html; } }

    /// <summary>二进制载荷（文本结果恒空）。</summary>
    public byte[] Data { get; } = new byte[0];

    public HtmlViewResult(string html) {
        this.Html = html;
        this.Headers = new WebHeaderCollection();
    }
}