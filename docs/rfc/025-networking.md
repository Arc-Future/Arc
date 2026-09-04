# RFC 025 网络协议层

## 背景

网络协议能力以 `std/Net`（`Arc.Net`）交付：HTTP/WebSocket/TCP/UDP 以 C# 惯用表面呈现、单一门面、版本自动协商、全异步。协议逻辑全部以 Arc 实现于 std，编译器核心零嵌协议。点对点网络（`Arc.Net.P2P`）为独立领域，见 [042](042-p2p.md)。

## 设计决策

### HttpClient 统一门面

HTTP/1.1 / HTTP/2 / HTTP/3 **不是三个并列公开客户端**，而是统一 `HttpClient` 门面内部的版本连接实现。用户从不直接接触 `Http11Connection`/`Http2Connection`/`Http3Connection`（内部连接层）。

```
HttpClient（薄门面）
  └─ HttpMessageHandler（抽象）
      ├─ DelegatingHandler（中间件基类）
      └─ SocketsHttpHandler（传输 handler · 版本协商调度）
          └─ HttpConnectionPoolManager
              └─ HttpConnectionPool
                  ├─ Http11Connection（HTTP/1.1）
                  ├─ Http2Connection（HTTP/2）
                  └─ Http3Connection（HTTP/3 / QUIC）
```

| 面 | 成员 |
|----|------|
| 门面 | `HttpClient()`/`HttpClient(HttpMessageHandler)`；`BaseAddress`/`Timeout`/`DefaultRequestHeaders`/`DefaultRequestVersion`/`DefaultVersionPolicy`/`CancelPendingRequests`/`Dispose` |
| 发送 | `SendAsync`/`GetAsync`/`PostAsync`/`PutAsync`/`PatchAsync`/`DeleteAsync` → `Task<HttpResponseMessage>`；`GetStringAsync`/`GetByteArrayAsync`/`GetStreamAsync` |
| 消息 | `HttpRequestMessage`（`Method`/`RequestUri`/`Headers`/`Content`/`Version`/`VersionPolicy`）；`HttpResponseMessage`（`StatusCode`（`HttpStatusCode` 枚举）/`ReasonPhrase`/`RequestMessage` 回链/`EnsureSuccessStatusCode`） |
| 版本协商 | `HttpVersionPolicy`（`RequestVersionOrLower`/`RequestVersionOrHigher`/`RequestVersionExact`）+ ALPN / Alt-Svc 自动协商；`HttpRequestMessage.Version` 为显式逃生口 |
| 内容 | `HttpContent` 抽象 + `StringContent`/`ByteArrayContent`/`StreamContent`/`FormUrlEncodedContent`/`MultipartFormDataContent`；`ReadAsStringAsync`/`ReadAsByteArrayAsync`/`ReadAsStreamAsync` |
| 中间件 | `DelegatingHandler` 链（认证/重试/日志可组合） |

- **单一惯用法**：HTTP 仅 `HttpClient` 一个公开入口；per-协议专用客户端不在本设计面内。HTTP 面全异步 `Task<T>`，不提供同步 HTTP 用户面。
- **协议能力**：HTTP/1.1 完整（chunked、keep-alive、状态行/头规范解析）；HTTP/2（h2c prior-knowledge + h2 via ALPN；HPACK + 流复用/流控）；HTTP/3（QUIC RFC 9000 + TLS 1.3 over QUIC RFC 9001 + HTTP/3 RFC 9114；QPACK 静态表 + 最小动态表）。
- **事件源（SSE）**不内嵌于 `HttpClient`——线协议解码由 `Arc.Net` 通用 `SseDecoder`（`std/Net/Core/Http/`，对标 .NET `System.Net.Http.SseParser` 分层）承担：基于 `StreamTransport` 异步面（含 chunked 经 `ChunkedStreamReader.ReadChunkAsync`）按 WHATWG event-stream 规范增量产出 `SseEvent{event,data,id,retry}` 异步序列（`IAsyncEnumerable<SseEvent>`，拉模型天然背压）；领域层（AI Provider 等）复用同一解码器做 SSE 字段 → 领域事件映射，禁各自重复实现行式解析。

### WebSocket

`WebSocketClient`（`std/Net/WebSocket/`）支持 `ws://`（无 TLS）经 `NetworkStream` 与 `wss://`（over TLS）经 `TlsClientSession` 字节层桥接。RFC 6455 Upgrade 握手（`Sec-WebSocket-Key`/`Accept`）、帧层（FIN/opcode/掩码）、文本帧往返、Ping/Pong、Close 握手。permessage-deflate 与分片续帧不在本设计面内。

```as
using Arc.Net;

using var ws = await WebSocket.ConnectAsync("ws://localhost:8080/chat");
await ws.SendAsync("hello");
string reply = await ws.ReceiveAsync();
```

### TCP / UDP

| 面 | 类型 | 说明 |
|----|------|------|
| TCP | `TcpClient`/`TcpListener` | 同步环回；`Start`→`Connect`→`Accept`→`Send`/`Receive` |
| 流 | `NetworkStream` | `Read(byte[], int, int)`/`Write(byte[], int, int)` 显式长度 byte[] 面；读返回实际字节数、EOF→0、部分读语义 |
| UDP | `UdpClient` | 数据报级 `Send(byte[], int, int, host, port)`（sendto，不 connect）/`Receive(byte[], int, int)`（recvfrom，返回实际字节数） |

### TLS 会话层归属（Arc.Net.Security）

TLS 会话层（`TlsClientSession`/`TlsServerSession`/`TlsNetworkStream`）归 `Arc.Net`——落 `std/Net/Core/Security/`，namespace `Arc.Net.Security`。对标 .NET 分层裁决：`System.Net.Security`（`SslStream`）归 Net 命名空间，`System.Security.Cryptography` 只含密码学原语与 X.509。

依赖方向：`Arc.Net` 声明依赖 `Arc.Security`（X.509 证书类型），`Arc.Security` 零网络引用——单向无环，两包均可独立构建。TLS 底座 ABI（`rt_crypto_tls_*`，vendored mbedTLS）不变。

**std 包依赖闭包校验的自身包豁免**：入口包源文件 `using` 自身包的根/子命名空间（如 `std/Net/Core` 文件 `using Arc.Net;` / `using Arc.Net.Security;`）是合法书写形态——目录与命名空间解耦下，子目录文件引用包根类型不可能（也无必要）向自身声明依赖，不计入未声明依赖。

## 边界

- 本文档讲**协议层**（Http/Tcp/Udp/WebSocket）；Web 应用框架（`WebApplication`、路由、SSR）见 [040 Web 框架与 SSR](040-web.md)。
- **点对点网络**（`Arc.Net.P2P`：对等身份、传输、DHT、NAT、中继、PubSub）为独立领域，见 [042](042-p2p.md)。
- TLS 会话层（`Arc.Net.Security`：握手、记录读写、明文面流）归本层；密码学原语与 X.509 证书见 [026 加密与安全](026-cryptography-security.md)。
- 字节缓冲与流语义见 [021 集合、IO 与文本](021-collections-io-text.md)。
- 并发调度/Reactor 见语言并发规范。

---

上一节：[024 并发集合](024-concurrent-collections.md) · 下一节：[026 加密与安全](026-cryptography-security.md)