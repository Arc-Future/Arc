// RFC 025 M4 + RFC 033 §1.0.1: Arc.Net — 流式内容（对齐 C# StreamContent）。
//
// 诚实边界：MVP 以字符串承载文本流（同步读）；真正 System.IO.Stream 抽象待
// 底层流管线就位后递升。异步当面待 §1.4。
namespace Arc.Net;

/// <summary>流式请求体内容（MVP：文本流承载，对齐 C# StreamContent）。</summary>
public class StreamContent : HttpContent {
    public StreamContent(string content) {
        this.Body = content;
        this.ContentType = "application/octet-stream";
    }

    public StreamContent(string content, string contentType) {
        this.Body = content;
        this.ContentType = contentType;
    }
}