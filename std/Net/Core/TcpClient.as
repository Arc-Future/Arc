// RFC 025 M4: Arc.Net — TcpClient 门面类。
//
// 对标 C# System.Net.Sockets.TcpClient（.NET 9）。TcpClient 是面向连接的
// TCP 客户端便捷封装，提供与底层 Socket 一致的完整选项控制和状态查询。
// 方法体为空 stub，codegen 拦截后直接发射 @rt_socket_* ABI 调用。
// `new TcpClient()` 由 emit_new 接线为 @rt_socket_create（非 calloc 空壳）。

namespace Arc.Net;

/// <summary>
/// TCP 客户端——提供面向连接的、可靠的字节流传输。
///
/// 对标 C# TcpClient。通过 Connect 建立连接，Send/Receive 传输数据，
/// 支持超时、NoDelay、缓冲区大小等完整选项。
/// </summary>
public class TcpClient : IDisposable {
    /// <summary>创建新的 TcpClient 实例（未连接）。使用默认 IPv4 TCP。</summary>
    [Builtin(ABI = "rt_socket_create")]
    public TcpClient() {}

    /// <summary>创建指定地址族的 TcpClient。</summary>
    [Builtin(ABI = "rt_socket_create")]
    public TcpClient(AddressFamily family) {}

    // ── 连接 ──

    /// <summary>连接到指定的远程主机和端口。</summary>
    [Builtin(ABI = "rt_socket_connect")]
    public bool Connect(string host, int port) { return false; }

    /// <summary>关闭 TCP 连接并释放资源。</summary>
    [Builtin(ABI = "rt_socket_close")]
    public void Close() {}

    // ── 数据传输 ──

    /// <summary>发送数据到已连接的远程端点。</summary>
    [Builtin(ABI = "rt_socket_send")]
    public int Send(string data) { return 0; }

    /// <summary>从已连接的远程端点接收数据（默认 4096 字节缓冲区）。</summary>
    [Builtin(ABI = "rt_socket_receive")]
    public string Receive() { return ""; }

    /// <summary>从已连接的远程端点接收指定大小的数据。</summary>
    [Builtin(ABI = "rt_socket_receive")]
    public string Receive(int bufferSize) { return ""; }

    // ── 原始字节面（S2 · RFC 033 §2.4）──

    /// <summary>
    /// 发送原始字节（显式长度，内部 0x00 不被 NUL 截断；HTTP/2 等二进制协议帧用）。
    /// 返回实际发送字节数（≤ count；失败返回 0），调用方需处理部分写。
    /// </summary>
    [Builtin(ABI = "rt_socket_send")]
    public int SendBytes(byte[] data, int offset, int count) { return 0; }

    /// <summary>
    /// 接收原始字节到缓冲区（显式长度，无 NUL 截断）。返回实际读入字节数
    /// （≤ count；EOF/超时返回 0）。TCP 部分读语义：调用方需循环直至读满或 0。
    /// </summary>
    [Builtin(ABI = "rt_net_recv")]
    public int ReceiveBytes(byte[] buffer, int offset, int count) { return 0; }

    // ── 异步操作（RFC 038 M2） ──

    /// <summary>异步连接到远程主机。基于 Reactor，不阻塞调用线程。</summary>
    /// <param name="host">远程主机名或 IP 地址。</param>
    /// <param name="port">远程端口号。</param>
    /// <returns>表示异步连接操作的 Task。</returns>
    [Builtin(ABI = "rt_socket_connect_async")]
    public Task ConnectAsync(string host, int port) { return null; }

    /// <summary>异步发送数据到已连接的远程端点。</summary>
    /// <param name="data">待发送的字符串数据。</param>
    /// <returns>表示异步发送操作的 Task&lt;int&gt;；完成后返回实际发送的字节数。</returns>
    [Builtin(ABI = "rt_socket_send_async")]
    public Task<int> SendAsync(string data) { return 0; }

    /// <summary>异步从已连接的远程端点接收数据（默认 4096 字节缓冲区）。</summary>
    /// <returns>表示异步接收操作的 Task&lt;string&gt;；完成后返回接收到的字符串。</returns>
    [Builtin(ABI = "rt_socket_receive_async")]
    public Task<string> ReceiveAsync() { return ""; }

    /// <summary>异步从已连接的远程端点接收指定大小的数据。</summary>
    /// <param name="bufferSize">期望读取的最大字节数。</param>
    /// <returns>表示异步接收操作的 Task&lt;string&gt;。</returns>
    [Builtin(ABI = "rt_socket_receive_async")]
    public Task<string> ReceiveAsync(int bufferSize) { return ""; }

    // ── 字节面异步（RFC 028 异步为主 · WebSocket wss TLS 密文含 0x00，string 面 NUL 截断不可用）──

    /// <summary>
    /// 异步发送原始字节（显式长度，内部 0x00 不被 NUL 截断）。基于 Reactor 提交 write。
    /// 返回实际发送字节数（≤ count；失败返回 0），调用方需处理部分写。
    /// </summary>
    [Builtin(ABI = "rt_socket_send_async")]
    public Task<int> SendBytesAsync(byte[] data, int offset, int count) { return 0; }

    /// <summary>
    /// 异步接收原始字节到缓冲区（显式长度，无 NUL 截断）。基于 Reactor 提交 read。
    /// 返回实际读入字节数（≤ count；EOF/超时返回 0）。TCP 部分读语义：调用方需循环直至读满或 0。
    /// </summary>
    [Builtin(ABI = "rt_socket_receive_bytes_async")]
    public Task<int> ReceiveBytesAsync(byte[] buffer, int offset, int count) { return 0; }

    // ── 状态 ──

    /// <summary>Socket 是否已连接到远程主机。</summary>
    [Builtin(ABI = "rt_socket_connected")]
    public bool Connected { get; }

    /// <summary>当前可读取的字节数。</summary>
    [Builtin(ABI = "rt_socket_available")]
    public int Available { get; }

    // ── 选项控制 ──

    /// <summary>获取/设置套接字发送缓冲区大小（字节）。</summary>
    [Builtin(ABI = "rt_socket_set_send_buf_size")]
    public void SetSendBufferSize(int size) {}

    /// <summary>获取/设置套接字接收缓冲区大小（字节）。</summary>
    [Builtin(ABI = "rt_socket_set_recv_buf_size")]
    public void SetReceiveBufferSize(int size) {}

    /// <summary>设置接收超时时间（毫秒）。0 表示无限等待。</summary>
    [Builtin(ABI = "rt_socket_set_recv_timeout")]
    public void SetReceiveTimeout(int milliseconds) {}

    /// <summary>设置发送超时时间（毫秒）。0 表示无限等待。</summary>
    [Builtin(ABI = "rt_socket_set_send_timeout")]
    public void SetSendTimeout(int milliseconds) {}

    /// <summary>启用/禁用 Nagle 算法。true 禁用（低延迟），false 启用（高吞吐）。</summary>
    [Builtin(ABI = "rt_socket_set_no_delay")]
    public void SetNoDelay(bool noDelay) {}

    // ── 生命周期 ──

    /// <summary>释放 TcpClient 持有的资源。</summary>
    public void Dispose() { this.Close(); }
}
