// RFC 048 §4: Arc.Net.Pipes — NamedPipeServerStream 门面（同步面，M0）。
//
// 对标 C# System.IO.Pipes.NamedPipeServerStream。方法体为空 stub，codegen
// 拦截后直接发射 @rt_pipe_* ABI 调用（builtin_dispatch BuiltinFacadeKind::Pipe
// → try_emit_pipe_method）。`new NamedPipeServerStream(...)` 由 emit_new 接线为
// @rt_pipe_server_create（非 calloc 空壳）。
// 字节流语义：无消息边界；Read 返回 0 = 对端有序关闭（统一 EOF）；
// Write 短写补尽，对端读端关闭时底层返回 0。单写者/单读者配对（RFC 048 §3.1-3）。

namespace Arc.Net.Pipes;

using Arc;
using Arc.IO;

/// <summary>
/// 命名管道服务端流——跨进程字节传输的本机 IPC 面。
///
/// 对标 C# NamedPipeServerStream。WaitForConnection 阻塞等待客户端接入；
/// 继承 <see cref="Arc.IO.Stream"/> 字节契约（管道不可寻址，CanSeek 恒 false）。
/// 双工语义：Windows 单内核对象；POSIX 以 name.in/out 双 FIFO 组装（对用户透明）。
/// </summary>
public class NamedPipeServerStream : Stream {
    /// <summary>创建服务端（默认单实例；缓冲 64KB）。</summary>
    /// <param name="pipeName">管道逻辑名（跨平台名字规范化见 RFC 048 §5.1-3）。</param>
    [Builtin(ABI = "rt_pipe_server_create")]
    public NamedPipeServerStream(string pipeName) {}

    /// <summary>创建服务端（指定最大实例数）。</summary>
    /// <param name="pipeName">管道逻辑名。</param>
    /// <param name="maxInstances">最大并行实例数（POSIX 为串行排队语义）。</param>
    [Builtin(ABI = "rt_pipe_server_create")]
    public NamedPipeServerStream(string pipeName, int maxInstances) {}

    /// <summary>阻塞等待客户端接入。已接入（ERROR_PIPE_CONNECTED / 竞态已连）视作成功。</summary>
    /// <returns>接入成功返回 true。</returns>
    [Builtin(ABI = "rt_pipe_server_wait_connect")]
    public bool WaitForConnection() { return false; }

    /// <summary>断开当前连接并复用实例（可再次 WaitForConnection）。</summary>
    [Builtin(ABI = "rt_pipe_server_disconnect")]
    public void Disconnect() {}

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
