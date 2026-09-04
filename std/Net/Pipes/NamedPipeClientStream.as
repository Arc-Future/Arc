// RFC 048 §4: Arc.Net.Pipes — NamedPipeClientStream 门面（同步面，M0）。
//
// 对标 C# System.IO.Pipes.NamedPipeClientStream。`new NamedPipeClientStream(...)`
// 由 emit_new 接线为 @rt_pipe_client_create（未连接壳）；Connect(timeoutMs) 走
// @rt_pipe_client_connect（ERROR_PIPE_BUSY → WaitNamedPipe 轮询重试至超时；
// POSIX 为 O_NONBLOCK open 轮询）。

namespace Arc.Net.Pipes;

using Arc;
using Arc.IO;

/// <summary>
/// 命名管道客户端流——跨进程字节传输的本机 IPC 面。
///
/// 对标 C# NamedPipeClientStream。Connect 阻塞接入（timeoutMs=-1 无限等待）；
/// 继承 <see cref="Arc.IO.Stream"/> 字节契约（管道不可寻址，CanSeek 恒 false）。
/// </summary>
public class NamedPipeClientStream : Stream {
    /// <summary>创建客户端（未连接；需 Connect 后方可读写）。</summary>
    /// <param name="pipeName">管道逻辑名（跨平台名字规范化见 RFC 048 §5.1-3）。</param>
    [Builtin(ABI = "rt_pipe_client_create")]
    public NamedPipeClientStream(string pipeName) {}

    /// <summary>接入服务端（ERROR_PIPE_BUSY 重试至超时；-1 = 无限等待）。</summary>
    /// <param name="timeoutMs">超时毫秒数（-1 = 无限等待）。</param>
    /// <returns>接入成功返回 true。</returns>
    [Builtin(ABI = "rt_pipe_client_connect")]
    public bool Connect(int timeoutMs) { return false; }

    /// <summary>是否处于已连接状态。</summary>
    [Builtin(ABI = "rt_pipe_is_connected")]
    public bool IsConnected { get { return false; } }

    /// <summary>关闭管道并释放底层资源。</summary>
    [Builtin(ABI = "rt_pipe_close")]
    public void Terminate() {}

    // ── Stream 字节契约（rt_pipe_read/write 直射；byte[] 载荷 + 显式 offset/count）──

    /// <summary>读取字节到缓冲区；返回实际读取数；0 = 对端有序关闭。</summary>
    [Builtin(ABI = "rt_pipe_read")]
    public override int Read(byte[] buffer, int offset, int count) { return 0; }

    /// <summary>将缓冲区字节写入管道（短写补写至尽）。</summary>
    [Builtin(ABI = "rt_pipe_write")]
    public override void Write(byte[] buffer, int offset, int count) {}

    // ── Stream 抽象面剩余成员（管道不可寻址，诚实最小实现）──

    /// <summary>管道可读。</summary>
    public override bool CanRead { get { return true; } }

    /// <summary>管道可写。</summary>
    public override bool CanWrite { get { return true; } }

    /// <summary>管道不支持定位。</summary>
    public override bool CanSeek { get { return false; } }

    /// <summary>不支持长度查询（字节流无界）。</summary>
    public override long Length { get { return 0; } }

    /// <summary>不支持位置。</summary>
    public override long Position { get { return 0; } set { } }

    /// <summary>不支持定位。</summary>
    public override long Seek(long offset, SeekOrigin origin) { return 0; }

    /// <summary>不支持设长。</summary>
    public override void SetLength(long value) {}

    /// <summary>无缓冲层，空实现。</summary>
    public override void Flush() {}

    /// <summary>释放底层管道资源。</summary>
    public override void Dispose() {
        this.Terminate();
    }
}
