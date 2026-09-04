// IWebResult —— Web 响应结果多态基面（RFC 040 §5）：HTML 页面 / 重定向 / 文件。
// 定义统一 HTTP 响应契约（状态码 / Content-Type / 头部 / 载荷），使任意结果都能被宿主
// 统一转写为 HTTP 响应（对齐 ASP.NET Core `IResult.ExecuteAsync(HttpContext)` 的
// "结果 → 响应" 职责，但以数据面契约形式表达）。
// IRequest<IWebResult> 作扩展响应类型，与 JSON API 走同一条 IMediator / IPipelineBehavior / DI 管道。
namespace Arc.Web;

using Arc.Net;

/// <summary>
/// Web 响应结果多态基面：IHtmlView（视图）/ IRedirectResult（重定向，PRG）/ IFileResult（文件/二进制）。
/// 每个结果暴露统一 HTTP 响应契约：状态码、Content-Type、响应头、文本/二进制载荷。
/// </summary>
public interface IWebResult {
    /// <summary>HTTP 状态码（200 为默认成功）。</summary>
    int StatusCode { get; }

    /// <summary>响应 Content-Type（如 "text/html; charset=utf-8"；无 body 语义时可为空）。</summary>
    string ContentType { get; }

    /// <summary>响应头集合（Location / Content-Disposition / Set-Cookie / 缓存头等）。</summary>
    WebHeaderCollection Headers { get; }

    /// <summary>是否为二进制载荷（true 读 Data；false 读 Body）。</summary>
    bool IsBinary { get; }

    /// <summary>文本响应体（IsBinary=false 时）。</summary>
    string Body { get; }

    /// <summary>二进制响应体（IsBinary=true 时）。</summary>
    byte[] Data { get; }
}