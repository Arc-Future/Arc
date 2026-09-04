// RFC 025 M4: Arc.Net — Socket 基类。
// 对标 C# System.Net.Sockets 命名空间（.NET 9）。Socket 是底层网络原语的门面类，
// 封装跨平台 socket 操作（Windows Winsock2 / Unix POSIX）。方法体为空 stub，
// codegen 拦截后直接发射 @rt_socket_* ABI 调用。

namespace Arc.Net;

/// <summary>
/// 网络 Socket 基类——封装底层操作系统 socket 句柄。
///
/// 对标 C# System.Net.Sockets.Socket（.NET 9）。提供连接管理 (Connect/Bind/Listen/Accept)、
/// 数据传输 (Send/Receive/Poll)、半关闭 (Shutdown) 和完整的选项控制。
/// TcpClient / TcpListener / UdpClient 均基于 Socket 构建。
///
/// 使用示例：
/// ```as
/// var s = new Socket(AddressFamily.InterNetwork, SocketType.Stream, ProtocolType.Tcp);
/// s.Connect("example.com", 80);
/// s.Send("GET / HTTP/1.1\r\nHost: example.com\r\n\r\n");
/// string data = s.Receive(4096);
/// s.Close();
/// ```
/// </summary>
public class Socket : IDisposable {
    /// <summary>创建新的 Socket 实例。</summary>
    [Builtin(ABI = "rt_socket_create")]
    public Socket(AddressFamily addressFamily, SocketType socketType, ProtocolType protocolType) {}

    // ── 连接管理 ──

    /// <summary>连接到指定的远程主机和端口。</summary>
    [Builtin(ABI = "rt_socket_connect")]
    public bool Connect(string host, int port) { return false; }

    /// <summary>绑定到本地端口。</summary>
    [Builtin(ABI = "rt_socket_bind")]
    public bool Bind(int port) { return false; }

    /// <summary>开始监听传入连接。</summary>
    [Builtin(ABI = "rt_socket_listen")]
    public bool Listen(int backlog) { return false; }

    /// <summary>接受一个传入连接，返回新的 Socket 实例。</summary>
    [Builtin(ABI = "rt_socket_accept")]
    public Socket Accept() { return null; }

    // ── 数据传输 ──

    /// <summary>发送数据到已连接的远程端点。</summary>
    [Builtin(ABI = "rt_socket_send")]
    public int Send(string data) { return 0; }

    /// <summary>从已连接的远程端点接收数据。</summary>
    [Builtin(ABI = "rt_socket_receive")]
    public string Receive(int bufferSize) { return ""; }

    /// <summary>从已连接的远程端点接收数据（默认 4096 字节缓冲区）。</summary>
    [Builtin(ABI = "rt_socket_receive")]
    public string Receive() { return ""; }

    // ── 异步数据传输（RFC 038 M2） ──

    /// <summary>异步连接到远程主机。基于 Reactor 提交 connect，不阻塞调用线程。</summary>
    /// <param name="host">远程主机名或 IP 地址。</param>
    /// <param name="port">远程端口号。</param>
    /// <returns>表示异步连接操作的 Task；完成后表示连接成功。</returns>
    [Builtin(ABI = "rt_socket_connect_async")]
    public Task ConnectAsync(string host, int port) { return null; }

    /// <summary>异步接受一个传入连接。基于 Reactor 提交 accept，不阻塞调用线程。</summary>
    /// <returns>表示异步接受操作的 Task&lt;Socket&gt;；完成后返回新的 Socket 实例。</returns>
    [Builtin(ABI = "rt_socket_accept_async")]
    public Task<Socket> AcceptAsync() { return null; }

    /// <summary>异步发送数据到已连接的远程端点。基于 Reactor 提交 write。</summary>
    /// <param name="data">待发送的字符串数据。</param>
    /// <returns>表示异步发送操作的 Task&lt;int&gt;；完成后返回实际发送的字节数。</returns>
    [Builtin(ABI = "rt_socket_send_async")]
    public Task<int> SendAsync(string data) { return 0; }

    /// <summary>异步从已连接的远程端点接收数据。基于 Reactor 提交 read。</summary>
    /// <param name="bufferSize">期望读取的最大字节数。</param>
    /// <returns>表示异步接收操作的 Task&lt;string&gt;；完成后返回接收到的字符串。</returns>
    [Builtin(ABI = "rt_socket_receive_async")]
    public Task<string> ReceiveAsync(int bufferSize) { return ""; }

    /// <summary>异步从已连接的远程端点接收数据（默认 4096 字节缓冲区）。</summary>
    [Builtin(ABI = "rt_socket_receive_async")]
    public Task<string> ReceiveAsync() { return ""; }

    // ── 字节面异步（RFC 028 异步为主 · WebSocket wss TLS 密文含 0x00，string 面 NUL 截断不可用）──

    /// <summary>
    /// 异步发送原始字节（显式长度，内部 0x00 不被 NUL 截断）。基于 Reactor 提交 write。
    /// 返回实际发送字节数（≤ count；失败返回 0），调用方需处理部分写。
    /// </summary>
    [Builtin(ABI = "rt_socket_send_async")]
    public Task<int> SendBytesAsync(byte[] data, int offset, int count) { return 0; }

    /// <summary>
    /// 异步接收原始字节到缓冲区（显式长度，无 NUL 截断）。基于 Reactor 提交 read。
    /// 返回实际读入字节数（≤ count；EOF/超时返回 0）。
    /// </summary>
    [Builtin(ABI = "rt_socket_receive_bytes_async")]
    public Task<int> ReceiveBytesAsync(byte[] buffer, int offset, int count) { return 0; }

    /// <summary>禁用指定方向的 Socket 操作（半关闭）。</summary>
    [Builtin(ABI = "rt_socket_shutdown")]
    public void Shutdown(SocketShutdown how) {}

    /// <summary>轮询 Socket 状态。</summary>
    /// <param name="microSeconds">等待微秒数；-1 表示无限等待。</param>
    /// <param name="mode">轮询模式（Read/Write/Error）。</param>
    /// <returns>指定状态就绪返回 true。</returns>
    [Builtin(ABI = "rt_socket_poll")]
    public bool Poll(int microSeconds, SelectMode mode) { return false; }

    // ── 状态属性 ──

    /// <summary>Socket 是否已连接。</summary>
    [Builtin(ABI = "rt_socket_connected")]
    public bool Connected { get; }

    /// <summary>当前可读取的字节数。</summary>
    [Builtin(ABI = "rt_socket_available")]
    public int Available { get; }

    // ── 选项控制 ──

    /// <summary>获取/设置套接字发送缓冲区大小。</summary>
    [Builtin(ABI = "rt_socket_set_send_buf_size")]
    public void SetSendBufferSize(int size) {}

    /// <summary>获取/设置套接字接收缓冲区大小。</summary>
    [Builtin(ABI = "rt_socket_set_recv_buf_size")]
    public void SetReceiveBufferSize(int size) {}

    /// <summary>设置接收超时时间。</summary>
    [Builtin(ABI = "rt_socket_set_recv_timeout")]
    public void SetReceiveTimeout(int milliseconds) {}

    /// <summary>设置发送超时时间。</summary>
    [Builtin(ABI = "rt_socket_set_send_timeout")]
    public void SetSendTimeout(int milliseconds) {}

    /// <summary>启用/禁用 Nagle 算法（NoDelay=true 禁用，减少小包延迟）。</summary>
    [Builtin(ABI = "rt_socket_set_no_delay")]
    public void SetNoDelay(bool noDelay) {}

    // ── 生命周期 ──

    /// <summary>关闭 Socket 连接并释放系统资源。</summary>
    [Builtin(ABI = "rt_socket_close")]
    public void Close() {}

    /// <summary>释放 Socket 持有的资源。</summary>
    public void Dispose() { this.Close(); }
}
