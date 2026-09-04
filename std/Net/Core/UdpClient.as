// RFC 025 M4: Arc.Net — UdpClient 门面类。
//
// 对标 C# System.Net.Sockets.UdpClient（.NET 9）。UdpClient 提供无连接 UDP
// 数据报收发能力。方法体为空 stub，codegen 拦截后直接发射 @rt_socket_* ABI 调用。
//
// 数据报级升级（RFC 033 §1.2.g · RFC 041 M0 前置 · 2026-08-05）：
//   收发面由 string 基础改为显式长度 byte[]（Send(byte[],int,int,string,int) /
//   Receive(byte[],int,int)），对标 C# UdpClient.Send/Receive(byte[]) 精华；
//   发送目标（远端 IP/端口）经 sendto 保留，本地绑定语义不变。
//   N3 教训：显式长度 + 内部 0x00 完整往返，彻底移除 string stub 的 NUL 截断隐患。

namespace Arc.Net;

/// <summary>
/// UDP 客户端——提供无连接的数据报收发。
///
/// 对标 C# UdpClient。适用于低延迟、容忍丢包的场景（DNS 查询、日志上报、实时音视频）。
/// 支持单播、广播和多播发送。
/// </summary>
public class UdpClient : IDisposable {
    /// <summary>创建新的 UdpClient 实例（使用默认 IPv4）。</summary>
    [Builtin(ABI = "rt_socket_create")]
    public UdpClient() {}

    /// <summary>创建指定地址族的 UdpClient。</summary>
    [Builtin(ABI = "rt_socket_create")]
    public UdpClient(AddressFamily family) {}

    /// <summary>创建 UdpClient 并绑定到指定本地端口。</summary>
    [Builtin(ABI = "rt_socket_bind")]
    public UdpClient(int port) {}

    // ── 数据收发（数据报级 · byte[] · 显式长度）──

    /// <summary>
    /// 向指定目标发送一个 UDP 数据报。
    ///
    /// 对标 C# UdpClient.Send(byte[], int, string, int) 精华。发送目标（远端
    /// IP/端口）经 sendto 直发，不改变本地绑定语义。载荷按显式 length 发送，
    /// 内部 0x00 完整往返，无 NUL 截断（N3 教训）。
    /// </summary>
    /// <param name="data">待发送的字节缓冲区。</param>
    /// <param name="offset">数据报起始偏移。</param>
    /// <param name="count">数据报字节数。</param>
    /// <param name="host">目标主机名或 IP 地址。</param>
    /// <param name="port">目标端口号。</param>
    /// <returns>实际发送的字节数（≤ count；失败返回 0）。</returns>
    [Builtin(ABI = "rt_socket_sendto_bytes")]
    public int Send(byte[] data, int offset, int count, string host, int port) { return 0; }

    /// <summary>
    /// 接收一个 UDP 数据报到调用方缓冲区。
    ///
    /// 对标 C# UdpClient.Receive(byte[]) 精华（buffer 形态 + 返回实际字节数）。
    /// 显式长度，内部 0x00 完整往返，无 NUL 截断（N3 教训）。
    /// </summary>
    /// <param name="buffer">接收缓冲区。</param>
    /// <param name="offset">写入起始偏移。</param>
    /// <param name="count">缓冲区可用字节数。</param>
    /// <returns>实际收到的数据报字节数（≤ count；失败/超时返回 0）。
    /// 数据报大于 count 时按 UDP 语义截断（仅保留前 count 字节）。</returns>
    [Builtin(ABI = "rt_socket_recvfrom_bytes")]
    public int Receive(byte[] buffer, int offset, int count) { return 0; }

    // ── 多播支持 ──

    /// <summary>绑定到本地端口（加入多播组前需要）。</summary>
    [Builtin(ABI = "rt_socket_bind")]
    public void JoinMulticastGroup(int port) {}

    /// <summary>离开多播组。</summary>
    [Builtin(ABI = "rt_socket_close")]
    public void DropMulticastGroup() {}

    // ── 选项 ──

    /// <summary>设置接收超时。</summary>
    [Builtin(ABI = "rt_socket_set_recv_timeout")]
    public void SetReceiveTimeout(int milliseconds) {}

    /// <summary>设置发送超时。</summary>
    [Builtin(ABI = "rt_socket_set_send_timeout")]
    public void SetSendTimeout(int milliseconds) {}

    /// <summary>当前可读字节数。</summary>
    [Builtin(ABI = "rt_socket_available")]
    public int Available { get; }

    // ── 生命周期 ──

    /// <summary>关闭 UDP Socket 并释放资源。</summary>
    [Builtin(ABI = "rt_socket_close")]
    public void Close() {}

    /// <summary>释放 UdpClient 持有的资源。</summary>
    public void Dispose() { this.Close(); }
}
