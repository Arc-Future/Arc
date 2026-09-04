# Arc.Net

## 概述

`Arc.Net` 是 Arc 的网络编程域，以 `std/Net/Core`（`Arc.Net`）为核心，`std/Net/P2P`（`Arc.Net.P2P`）、`std/Net/Pipes`（`Arc.Net.Pipes`）、`std/Net/Grpc`（`Arc.Net.Grpc`）为显式依赖子库交付。设计目标：HTTP/WebSocket/TCP/UDP 以 C# 惯用表面呈现、单一门面、版本自动协商、全异步；P2P 以独立栈交付对等身份与传输。协议逻辑全部以 Arc 实现于 std，编译器核心零嵌协议。

本册讲如何使用 `Arc.Net` 开发网络应用；Web 应用框架（`WebApplication`、路由、SSR）见 [web.md](web.md)。

## 快速开始

### HttpClient —— 统一门面

HTTP/1.1 / HTTP/2 / HTTP/3 **不是三个并列公开客户端**，而是统一 `HttpClient` 门面内部的版本连接实现。用户从不直接接触内部连接层。

```as
using Arc.Net;

using HttpClient client = new HttpClient();
client.BaseAddress = new Uri("https://api.example.com");
client.DefaultRequestHeaders.Add("Accept", "application/json");

// 强类型读取
string json = await client.GetStringAsync("/users/1");

// 结构化往返
HttpResponseMessage resp = await client.PostAsync(
    "/users",
    new StringContent("{\"name\":\"Alice\"}", System.Text.Encoding.UTF8, "application/json"));
resp.EnsureSuccessStatusCode();
```

### WebSocket

```as
using Arc.Net;

using var ws = await WebSocket.ConnectAsync("ws://localhost:8080/chat");
await ws.SendAsync("hello");
string reply = await ws.ReceiveAsync();
```

### TCP / UDP

```as
using Arc.Net;

// TCP 客户端
TcpClient client = new TcpClient();
client.Connect("127.0.0.1", 5001);
NetworkStream stream = client.GetStream();
stream.Write(data, 0, data.Length);

// UDP 数据报
UdpClient udp = new UdpClient();
udp.Send(bytes, 0, bytes.Length, "127.0.0.1", 5002);
int n = udp.Receive(buffer, 0, buffer.Length);   // 返回实际字节数
```

## 核心 API

### HttpClient 门面

| 面 | 成员 |
|----|------|
| 门面 | `HttpClient()`/`HttpClient(HttpMessageHandler)`；`BaseAddress`/`Timeout`/`DefaultRequestHeaders`/`DefaultRequestVersion`/`DefaultVersionPolicy` |
| 发送 | `SendAsync`/`GetAsync`/`PostAsync`/`PutAsync`/`PatchAsync`/`DeleteAsync` → `Task<HttpResponseMessage>`；`GetStringAsync`/`GetByteArrayAsync`/`GetStreamAsync` |
| 消息 | `HttpRequestMessage`（`Method`/`RequestUri`/`Headers`/`Content`/`Version`）；`HttpResponseMessage`（`StatusCode`/`ReasonPhrase`/`EnsureSuccessStatusCode`） |
| 版本协商 | `HttpVersionPolicy`（`RequestVersionOrLower`/`RequestVersionOrHigher`/`RequestVersionExact`）+ ALPN/Alt-Svc 自动协商 |
| 内容 | `HttpContent` 抽象 + `StringContent`/`ByteArrayContent`/`StreamContent`/`FormUrlEncodedContent`/`MultipartFormDataContent` |
| 中间件 | `DelegatingHandler` 链（认证/重试/日志可组合） |

**单一惯用法**：HTTP 仅 `HttpClient` 一个公开入口；HTTP 面全异步 `Task<T>`。SSE 线协议解码由 `Arc.Net` 通用 `SseDecoder`（对标 .NET `System.Net.Http.SseParser` 分层）承担——基于 `StreamTransport` 异步面按 WHATWG event-stream 规范增量产出 `SseEvent` 异步序列，不内嵌于 `HttpClient`；领域层（AI Provider 等）复用同一解码器做领域事件映射。

### WebSocket

| 类型 | 说明 |
|------|------|
| `WebSocket.ConnectAsync(uri)` | `ws://`（无 TLS）经 `NetworkStream`，`wss://`（over TLS）经 `TlsClientSession` 桥接 |
| `SendAsync(text)` | 文本帧发送 |
| `ReceiveAsync()` | 文本帧接收 |

RFC 6455 Upgrade 握手、帧层（FIN/opcode/掩码）、Ping/Pong、Close 握手均支持。

### TCP / UDP

| 面 | 类型 | 说明 |
|----|------|------|
| TCP | `TcpClient`/`TcpListener` | `Start`→`Connect`→`Accept`→`Send`/`Receive` |
| 流 | `NetworkStream` | `Read(byte[], int, int)`/`Write(byte[], int, int)` 显式长度 byte[] 面；读返回实际字节数、EOF→0、部分读语义 |
| UDP | `UdpClient` | 数据报级 `Send(byte[], int, int, host, port)`（sendto，不 connect）/`Receive(byte[], int, int)`（recvfrom，返回实际字节数） |

### gRPC（Arc.Net.Grpc）

`Arc.Net.Grpc` 提供 gRPC 框架：`GrpcChannel`（客户端）/`GrpcServer`（服务端）+ 四种调用形态（unary / server-streaming / client-streaming / bidi-streaming）。服务以显式 `MethodDefinition` + 处理器注册，无代码生成器。

```as
using Arc.Net.Grpc;

GrpcServer server = new GrpcServer();
server.AddService(new GreeterService());          // 注册服务定义
await server.StartAsync("tcp://localhost:50051");
```

## Arc.Net.P2P

`Arc.Net.P2P` 独立子库交付对等身份、地址、拓扑与传输。P2P 面为异步（`DialAsync` 等），与 HTTP 全异步一致。

### 身份与地址

```as
using Arc.Net.P2P;

// 从公钥派生 PeerId
PeerId peerId = PeerId.FromPublicKey(publicKey);

// 解析 Multiaddr
Multiaddr addr = Multiaddr.Parse("/ip4/127.0.0.1/tcp/5001/p2p/Qm...");
```

| 面 | 类型 |
|----|------|
| 身份/地址 | `PeerId`/`Multiaddr`/`MultiaddrProtocol`/`CID`/`PeerKey`/`SignedEnvelope`/`PeerRecord` |
| 存储/拓扑 | `IPeerStore`/`InMemoryPeerStore`/`FullMeshTopology`/`ITopology` |
| 传输/连接 | `ITransport`/`IConnection`/`IStream`/`TCPTransport`（Noise 安全握手） |
| 发现/穿透 | `IDiscovery`/`STUNClient`/`STUNResult`/`NATType`/`ICEAgent`/`ICECandidate`/`AutoNAT`/`NATStatus` |
| 中继/消息 | `CircuitRelay`/`RelayServer`/`RelayReservation`/`P2PMessage`/`MessageType`/`IPubSub`/`ISubscription`/`GossipSubRouter` |

### 拨号与连接

```as
using Arc.Net.P2P;

TCPTransport transport = new TCPTransport();
IConnection conn = await transport.DialAsync(addr, ct);
```

**对等身份**：Ed25519 派生 `PeerId`；传输保密经 Noise 握手。**协议内部机制**（`StreamMuxer`/`HeartbeatService`/`ConnectionMonitor`/`IdentifyProtocol`/内部拓扑）为 `internal`；用户操作结果句柄（`STUNResult`/`ICECandidate`/`PeerConnectionState`）为 `public`。

### P2P 能力面

`Arc.Net.P2P` 对标 libp2p 能力面，覆盖传输（TCP + 流复用 + 协商 + QUIC + WebSocket）、安全（Noise + TLS 1.3）、身份、发现（mDNS + Kademlia DHT）、NAT（STUN/TURN/ICE + AutoNAT + DCUtR 打洞）、中继（Circuit Relay v2）、PubSub（gossipsub）。协议层全部纯 Arc 实现，密码学/传输热路径复用既有 C 底座。互操作级验收为核心判据（本地闭环 ≠ 生态互操作）。

> 各能力面按依赖链逐面交付；未交付面以显式失败表达，不静默假绿。浏览器传输面（WebTransport/WebRTC）**当前不提供**，桌面 AOT 优先。

## 边界

- 本册讲**协议层**（Http/Tcp/Udp/WebSocket/P2P）；Web 应用框架（`WebApplication`、路由、SSR）见 [web.md](web.md)。
- TLS 1.3 会话层见 `Arc.Net.Security`（`std/Net/Core/Security/`，本包族）；X.509 与密码学原语见 `Arc.Security`。
- 字节缓冲与流语义见标准库基础面 `Arc.IO`。
- 并发调度/Reactor 见语言并发规范。

---

上一节：[web.md](web.md) · 下一节：[di.md](di.md)