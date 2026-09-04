// RFC 025 M4: Arc.Net — HTTP 方法枚举（对标 C# System.Net.Http.HttpMethod）。
//
// Arc enum 使用显式判别式精确对应 HTTP 方法编号。

namespace Arc.Net;

/// <summary>HTTP 请求方法。</summary>
public enum HttpMethod {
    GET = 0,
    POST = 1,
    PUT = 2,
    DELETE = 3,
    HEAD = 4,
    OPTIONS = 5,
    PATCH = 6,
    CONNECT = 7,
    TRACE = 8,
}
