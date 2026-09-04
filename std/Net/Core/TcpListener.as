// RFC 025 M4: Arc.Net — TcpListener 门面类。
//
// 对标 C# System.Net.Sockets.TcpListener（.NET 9）。监听指定端口的 TCP 连接。
// 方法体为空 stub，codegen 拦截后直接发射 @rt_socket_* ABI 调用。
// `new TcpListener()` 由 emit_new 接线为 @rt_socket_create（非 calloc 空壳）。

namespace Arc.Net;

/// <summary>
/// TCP 监听器——在指定端口上监听传入的 TCP 连接请求。
///
/// 对标 C# TcpListener。通过 Start 绑定端口并开始监听，AcceptTcpClient/AcceptSocket
/// 阻塞等待客户端连接，Pending 查询是否有等待中的连接。
/// </summary>
public class TcpListener : IDisposable {
    /// <summary>创建新的 TcpListener 实例（未绑定）。使用默认 IPv4。</summary>
    [Builtin(ABI = "rt_socket_create")]
    public TcpListener() {}

    /// <summary>创建指定地址族的 TcpListener。</summary>
    [Builtin(ABI = "rt_socket_create")]
    public TcpListener(AddressFamily family) {}

    // ── 监听控制 ──

    /// <summary>绑定到本地端口并开始监听。</summary>
    /// <param name="port">本地监听端口号。</param>
    /// <returns>启动成功返回 true。</returns>
    [Builtin(ABI = "rt_socket_bind")]
    public bool Start(int port) { return false; }

    /// <summary>绑定到本地端口并开始监听（指定等待队列长度）。</summary>
    /// <param name="port">本地监听端口号。</param>
    /// <param name="backlog">等待队列的最大长度。</param>
    /// <returns>启动成功返回 true。</returns>
    [Builtin(ABI = "rt_socket_bind")]
    public bool Start(int port, int backlog) { return false; }

    /// <summary>判断是否有等待中的连接请求。</summary>
    /// <returns>有待处理连接返回 true。</returns>
    [Builtin(ABI = "rt_socket_poll")]
    public bool Pending() { return false; }

    /// <summary>接受一个传入的客户端连接，返回 TcpClient。</summary>
    /// <returns>代表客户端连接的新 TcpClient；无连接时返回 null。</returns>
    [Builtin(ABI = "rt_socket_accept")]
    public TcpClient AcceptTcpClient() { return null; }

    /// <summary>接受一个传入连接，返回底层 Socket。</summary>
    /// <returns>代表客户端连接的 Socket；无连接时返回 null。</returns>
    [Builtin(ABI = "rt_socket_accept")]
    public Socket AcceptSocket() { return null; }

    // ── 异步接受（RFC 038 M2） ──

    /// <summary>异步接受一个传入的客户端连接。基于 Reactor，不阻塞调用线程。</summary>
    /// <returns>表示异步接受操作的 Task&lt;TcpClient&gt;；完成后返回新的 TcpClient。</returns>
    [Builtin(ABI = "rt_socket_accept_async")]
    public Task<TcpClient> AcceptTcpClientAsync() { return null; }

    /// <summary>异步接受一个传入连接并返回底层 Socket。</summary>
    /// <returns>表示异步接受操作的 Task&lt;Socket&gt;；完成后返回新的 Socket。</returns>
    [Builtin(ABI = "rt_socket_accept_async")]
    public Task<Socket> AcceptSocketAsync() { return null; }

    /// <summary>停止监听并释放资源。</summary>
    [Builtin(ABI = "rt_socket_close")]
    public void Stop() {}

    // ── 状态 ──

    /// <summary>是否正在监听。</summary>
    // codegen 无 get_Active 直射（try_emit_socket_method 仅 Connected/Available），
    // 须保留显式死代码体（[Builtin] 自动属性未拦截将读空 backing field 恒错）。
    [Builtin(ABI = "rt_socket_connected")]
    public bool Active { get { return false; } }

    // ── 生命周期 ──

    /// <summary>释放 TcpListener 持有的资源。</summary>
    public void Dispose() { this.Stop(); }
}
