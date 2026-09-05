// Http2Request —— 拆分自 Http2Types.as（一文件一公开类型）。
namespace Arc.Net;
using Arc.Collections;

/// <summary>HTTP/2 请求（GET/HEAD 等无体请求；可选 Body）。传输原语数据载体（跨包消费需 public）。</summary>
public class Http2Request {
    /// <summary>请求方法（如 "GET"）；空串视为 GET。</summary>
    public string Method;

    /// <summary>请求路径（如 "/index.html?a=1"）；空串视为 "/"。</summary>
    public string Path;

    /// <summary>用户头（伪头与 host 由客户端统一构造，不可覆写）。</summary>
    public Http2HeaderList Headers;

    /// <summary>请求体（可选）；空数组 = 无体（HEADERS 即带 END_STREAM）。</summary>
    public byte[] Body;

    /// <summary>构造请求。</summary>
    public Http2Request(string method, string path) {
        Method = method;
        Path = path;
        Headers = new Http2HeaderList();
        Body = Http2ByteUtils.ZeroBytes(0);
    }
}
