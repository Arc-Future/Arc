// 标准库就绪 P0：FileStream — 文件字节流（Stable 最小同步面）。
// 对标 C# System.IO.FileStream；底层经 rt_file_stream_* ABI。

namespace Arc.IO;

/// <summary>
/// 文件字节流——对打开的文件提供 <see cref="Stream"/> 同步读写与定位。
/// </summary>
/// <remarks>
/// Stable：OpenRead / OpenWrite / Create + Read / Write / Seek / Position / Length /
/// Flush / Dispose。CopyTo 继承自 <see cref="Stream"/>。
/// 异步面（ReadAsync / WriteAsync / FlushAsync）经文件 I/O 线程池真异步
/// （阻塞操作卸载至专用池线程，完成后向事件循环投递完成信号唤醒 await，
/// 不阻塞调用线程；语义对标 .NET FileStream 默认路径）。
/// <para>
/// 布局：唯一字段 <c>_handle</c> 位于对象头后 offset 16；codegen 发射 <c>rt_file_stream_*</c>。
/// </para>
/// </remarks>
public class FileStream : Stream {
    private int _handle;

    /// <summary>打开文件。mode：0 只读，1 写入截断，2 创建截断。</summary>
    [Builtin(ABI = "rt_file_stream_open")]
    public FileStream(string path, int mode) {
        _handle = 0;
    }

    /// <summary>以只读方式打开现有文件。</summary>
    public static FileStream OpenRead(string path) {
        return new FileStream(path, 0);
    }

    /// <summary>以写入方式打开或创建文件（截断）。</summary>
    public static FileStream OpenWrite(string path) {
        return new FileStream(path, 1);
    }

    /// <summary>创建或覆盖文件用于写入。</summary>
    public static FileStream Create(string path) {
        return new FileStream(path, 2);
    }

    [Builtin(ABI = "rt_file_stream_can_read")]
    public override bool CanRead {
        get { return false; }
    }

    [Builtin(ABI = "rt_file_stream_can_write")]
    public override bool CanWrite {
        get { return false; }
    }

    [Builtin(ABI = "rt_file_stream_can_seek")]
    public override bool CanSeek {
        get { return false; }
    }

    [Builtin(ABI = "rt_file_stream_get_length")]
    public override long Length {
        get { return 0; }
    }

    public override long Position {
        get { return _getPosition(); }
        set { _setPosition(value); }
    }

    [Builtin(ABI = "rt_file_stream_get_position")]
    private long _getPosition() { return 0; }

    [Builtin(ABI = "rt_file_stream_set_position")]
    private void _setPosition(long value) { }

    [Builtin(ABI = "rt_file_stream_read")]
    public override int Read(byte[] buffer, int offset, int count) { return 0; }

    [Builtin(ABI = "rt_file_stream_write")]
    public override void Write(byte[] buffer, int offset, int count) { }

    [Builtin(ABI = "rt_file_stream_seek")]
    public override long Seek(long offset, SeekOrigin origin) { return 0; }

    [Builtin(ABI = "rt_file_stream_set_length")]
    public override void SetLength(long value) { }

    [Builtin(ABI = "rt_file_stream_flush")]
    public override void Flush() { }

    // ── 真异步面（文件 I/O 线程池卸载 + EventLoop 完成投递，对标 C# Stream 异步精华面）──
    //
    // 阻塞读写/刷新卸载至文件 I/O 专用线程池（独立于 Task.Run 默认池），调用线程
    // 提交后立即返回 Pending Task；池线程完成后经 rt_task_complete → 事件循环
    // 就绪队列唤醒 await。非 sync-over-async：阻塞发生在池线程。取消为提交前
    // 预检（已取消返回已取消 Task，不占池线程）；进入池线程后不可中止（CRT I/O
    // 无取消原语，与 .NET FileStream 非重叠路径一致）。位置语义与同步面同源
    // （同一 FILE* 句柄），同步/异步混用不分裂 Position。

    /// <summary>
    /// 异步读取至多 <paramref name="count"/> 字节到 <paramref name="buffer"/>
    /// （自 <paramref name="offset"/> 起）；返回实际读取字节数；EOF 返回 0。
    /// </summary>
    /// <param name="cancellationToken">取消令牌：提交前已取消则任务直接置为已取消。</param>
    [Builtin(ABI = "rt_file_stream_read_async")]
    public override Task<int> ReadAsync(byte[] buffer, int offset, int count, CancellationToken cancellationToken = default) { return null; }

    /// <summary>异步全量写入 <paramref name="buffer"/> 中 <paramref name="count"/> 字节（自 <paramref name="offset"/> 起）。</summary>
    /// <param name="cancellationToken">取消令牌：提交前已取消则任务直接置为已取消。</param>
    [Builtin(ABI = "rt_file_stream_write_async")]
    public override Task WriteAsync(byte[] buffer, int offset, int count, CancellationToken cancellationToken = default) { return null; }

    /// <summary>异步刷新写入缓冲（对齐 <see cref="Flush"/>）。</summary>
    /// <param name="cancellationToken">取消令牌：提交前已取消则任务直接置为已取消。</param>
    [Builtin(ABI = "rt_file_stream_flush_async")]
    public override Task FlushAsync(CancellationToken cancellationToken = default) { return null; }

    [Builtin(ABI = "rt_file_stream_close")]
    public override void Dispose() { }
}
