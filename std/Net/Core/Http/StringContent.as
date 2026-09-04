// RFC 025 M4: Arc.Net — 字符串请求体内容。
namespace Arc.Net;

/// <summary>字符串请求体。</summary>
public class StringContent : HttpContent {
    public StringContent(string content) {
        this.Body = content;
        this.ContentType = "text/plain";
    }

    public StringContent(string content, string contentType) {
        this.Body = content;
        this.ContentType = contentType;
    }
}
