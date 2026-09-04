// Arc.Net — 服务端监听协议声明（对齐 ASP.NET Core Kestrel HttpProtocols）。
//
// 宿主（Arc.Web WebApplication）经 ListenAnyIP/Localhost/Listen(+Action<ListenOptions>
// 配置回调设 Protocols) 声明端点协议；组合值为位掩码（Http1AndHttp2 = Http1|Http2），
// 保持与 ASP.NET Core 枚举语义一致。
namespace Arc.Net;

/// <summary>
/// 监听端点启用的 HTTP 协议（对齐 Microsoft.AspNetCore.Server.Kestrel.Core.HttpProtocols）。
/// </summary>
public enum HttpProtocols {
    /// <summary>未启用任何协议。</summary>
    None = 0,
    /// <summary>HTTP/1.1。</summary>
    Http1 = 1,
    /// <summary>HTTP/2（h2c 明文或 TLS ALPN 视传输底座）。</summary>
    Http2 = 2,
    /// <summary>HTTP/1.1 与 HTTP/2（端口协议探测）。</summary>
    Http1AndHttp2 = 3,
    /// <summary>HTTP/3（QUIC；传输底座接入后生效）。</summary>
    Http3 = 4,
    /// <summary>HTTP/1.1、HTTP/2 与 HTTP/3。</summary>
    Http1AndHttp2AndHttp3 = 7,
}
