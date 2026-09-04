// RFC 025 M4 / RFC 033 S3: Arc.Net — 传输抽象（明文 TCP 与 TLS 明文面统一字面契约）。
//
// 对标 C# System.Net.Sockets.NetworkStream 的精华面（RFC 003 单一惯用法）：
// HTTP/SSE 解析（ChunkedStreamReader / AI.DeepSeek 等）只依赖 string 级 I/O
// （ReadLine / ReadString / ReadToEnd / Write(string)）与 byte[] 面
// （Read / Write）。本抽象基类固化为传输载体契约，使开发者可对同一套解析逻辑
// 透明切换明文（NetworkStream）与 TLS（TlsNetworkStream）传输——深层 https 直连
// 复用既有 HTTP 逻辑而不改写。
//
// 设计对齐 `AIModelProvider`（抽象基类 + 公开契约）先例：宿主以基类引用存储，
// 规避 interface-typed 实例字段的异步调用运行时 AV（RFC 006 语言缺口）。
// 传输载体 = string 面（N3 · NUL 终止）与 byte[] 面（显式长度）并存——
// 文本协议（HTTP 头/SSE 事件）走 string 面；二进制载荷走 byte[] 面。

namespace Arc.Net;

using Arc.IO;

/// <summary>
/// 流式传输载体契约——明文 TCP 与 TLS 明文面统一抽象。
/// 派生实现在 `Arc.Net`（NetworkStream）与 `Arc.Security.Cryptography`
/// （TlsNetworkStream）子命名空间，遵循「基类在上、派生在下」命名空间分层原则。
/// </summary>
public abstract class StreamTransport {
    /// <summary>读取至多 <paramref name="count"/> 字节到缓冲区；返回实际字节数；EOF 返回 0。</summary>
    public abstract int Read(byte[] buffer, int offset, int count);

    /// <summary>全量写入 <paramref name="count"/> 字节；失败抛 IOException。</summary>
    public abstract void Write(byte[] buffer, int offset, int count);

    /// <summary>读取至多 <paramref name="bufferSize"/> 字节为字符串；EOF 返回 null。</summary>
    public abstract string ReadString(int bufferSize);

    /// <summary>全量写入字符串；返回实际写入字节数，失败返回 0。</summary>
    /// <remarks>与 <see cref="ReadString"/> 对称的 string 级写面（byte[] 面为
    /// <see cref="Write(byte[], int, int)"/>）。分离命名以规避虚表按名去重时
    /// 重载同名虚方法冲突（RFC 025 M4 虚方法槽按方法名索引）。</remarks>
    public abstract int WriteString(string data);

    /// <summary>读取一行（至 \n，剥离尾部 \r）；EOF 返回 null。</summary>
    public abstract string ReadLine();

    /// <summary>读取全部剩余数据直到连接关闭。</summary>
    public abstract string ReadToEnd();

    /// <summary>刷新写入缓冲并释放读取缓冲。</summary>
    public abstract void Flush();

    /// <summary>关闭底层传输。</summary>
    public abstract void Close();

    /// <summary>
    /// 将本传输从当前位置起的剩余字节拷贝到 <paramref name="destination"/>
    /// （对齐 C# Stream.CopyTo，见 <see cref="Arc.IO.Stream.CopyTo"/>）。
    /// 经 byte[] 读面 <see cref="Read(byte[], int, int)"/> 与目标写面循环，
    /// 固定 64 字节缓冲（诚实默认真体，非 stub）。拷贝后本传输 Position 移至末尾。
    /// </summary>
    public void CopyTo(Stream destination) {
        if (destination == null) {
            throw new ArgumentNullException("destination");
        }
        byte[] buffer = [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
        ];
        int n = this.Read(buffer, 0, 64);
        while (n > 0) {
            destination.Write(buffer, 0, n);
            n = this.Read(buffer, 0, 64);
        }
    }

    // ── 真异步方法面（RFC 028 异步为主 · Reactor 真异步，不阻塞调用线程）──
    //
    // 与同步面一一对应（byte[] + string 级），实现经 Reactor 提交 read/write await
    // （NetworkStream/TlsNetworkStream 分别代理 TcpClient.SendBytesAsync/ReceiveBytesAsync
    // 与 TlsClientSession.ReadAsync/WriteAsync），使 HTTP/SSE 解析可在真异步下增量读。
    // 命名以 Async 后缀消歧重载（虚表按方法名去重，RFC 025 M4 惯例）。

    /// <summary>异步读取至多 <paramref name="count"/> 字节到缓冲区；返回实际字节数；EOF 返回 0。</summary>
    public abstract Task<int> ReadBytesAsync(byte[] buffer, int offset, int count);

    /// <summary>异步全量写入 <paramref name="count"/> 字节；失败抛 IOException。</summary>
    public abstract Task WriteBytesAsync(byte[] buffer, int offset, int count);

    /// <summary>异步读取至多 <paramref name="bufferSize"/> 字节为字符串；EOF 返回 null。</summary>
    public abstract Task<string> ReadStringAsync(int bufferSize);

    /// <summary>异步全量写入字符串；返回实际写入字节数，失败返回 0。</summary>
    public abstract Task<int> WriteStringAsync(string data);

    /// <summary>异步读取一行（至 \n，剥离尾部 \r）；EOF 返回 null。</summary>
    public abstract Task<string> ReadLineAsync();

    /// <summary>异步读取全部剩余数据直到连接关闭。</summary>
    public abstract Task<string> ReadToEndAsync();
}