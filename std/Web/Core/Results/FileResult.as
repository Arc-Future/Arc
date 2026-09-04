// FileResult —— IFileResult 默认实现（RFC 040 §5）：文件/二进制响应结果。
// 实现 IWebResult 统一 HTTP 契约：200 + Content-Type + Content-Disposition 头（文件名非空时）
// + 二进制载荷。契约成员使用 getter-only auto-property（构造期赋值/初值），
// 消除显式 getter + backing field 冗余；getter 零成本读字段。
namespace Arc.Web;

using Arc.Net;

/// <summary>文件结果实现：Handler 经 PageHandler.File(data, contentType[, fileName]) 返回。</summary>
public class FileResult : IFileResult {
    /// <summary>下载文件名（空表示内联展示）。</summary>
    public string FileName { get; }

    /// <summary>MIME 类型。</summary>
    public string ContentType { get; }

    /// <summary>文件字节载荷。</summary>
    public byte[] Data { get; }

    /// <summary>响应头集合（含 Content-Disposition）。</summary>
    public WebHeaderCollection Headers { get; }

    /// <summary>HTTP 状态码（200 OK）。</summary>
    public int StatusCode { get; } = 200;

    /// <summary>文件为二进制载荷。</summary>
    public bool IsBinary { get; } = true;

    /// <summary>文本响应体（二进制结果恒空）。</summary>
    public string Body { get; } = "";

    public FileResult(byte[] data, string contentType, string fileName) {
        this.Data = data;
        this.ContentType = contentType;
        this.FileName = fileName;
        this.Headers = new WebHeaderCollection();
        if (fileName != "") {
            this.Headers.Add("Content-Disposition", "attachment; filename=\"" + fileName + "\"");
        }
    }
}