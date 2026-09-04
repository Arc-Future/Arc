// RFC 025 M4 + RFC 033 §1.0.1: Arc.Net — HTTP 请求/响应内容抽象基类。
//
// 对齐 C# System.Net.Http.HttpContent（.NET 9）精华：Body + ContentType 载体，
// 提供同步读取面 ReadAsString()/ReadAsByteArray() 与异步读取面
// ReadAsStringAsync()/ReadAsByteArrayAsync()（§1.4 · Task.FromResult 包裹 P1 同步）。
namespace Arc.Net;

using Arc.IO;
using Arc.Text;

/// <summary>HTTP 请求/响应内容抽象——请求体/响应体的载体（对齐 C# HttpContent）。</summary>
public class HttpContent {
    public string Body;
    public string ContentType;

    /// <summary>流式响应的活动传输载体（null = 已全缓冲；供 ReadAsStream 暴露活传输）。</summary>
    public StreamTransport LiveTransport;

    public HttpContent() {
        this.Body = "";
        this.ContentType = "";
        this.LiveTransport = null;
    }

    /// <summary>将内容读为字符串（同步当面）。</summary>
    public string ReadAsString() {
        return this.Body;
    }

    /// <summary>将内容读为字节数组。</summary>
    public byte[] ReadAsByteArray() {
        return Encoding.GetBytes(this.Body);
    }

    /// <summary>将内容读为字节流（对齐 C# ReadAsStream）。流式响应返回活动
    /// StreamTransport（可增量读）；全缓冲内容返回 MemoryStream 承载。</summary>
    public StreamTransport ReadAsStream() {
        if (this.LiveTransport != null) {
            return this.LiveTransport;
        }
        return new MemoryStreamTransport(this.ReadAsByteArray());
    }

    /// <summary>将内容读为字符串（异步 Task 面）。</summary>
    public Task<string> ReadAsStringAsync() {
        return Task.FromResult(this.ReadAsString());
    }

    /// <summary>将内容读为字节数组（异步 Task 面）。</summary>
    public Task<byte[]> ReadAsByteArrayAsync() {
        return Task.FromResult(this.ReadAsByteArray());
    }

    /// <summary>将内容读为字节流（异步 Task 面，对齐 C# ReadAsStreamAsync）。</summary>
    public Task<StreamTransport> ReadAsStreamAsync() {
        return Task.FromResult(this.ReadAsStream());
    }
}
