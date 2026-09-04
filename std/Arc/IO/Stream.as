// RFC 027 M0: 标准库本地化 — 流基类 Stream。
//
// 对标 C# System.IO.Stream。L2 Stable：抽象同步面 + CopyTo 默认实现 +
// ReadAsync/WriteAsync/FlushAsync 异步虚面（RFC 038 M2，默认同步完成）。
// 具体子类：MemoryStream（含 ToArray）/ FileStream（标准库就绪 P0，同步面物化）。

namespace Arc.IO;

/// <summary>
/// 流基类——提供字节序列的通用视图（同步抽象面 + 异步虚面）。
///
/// 子类须实现读写/定位/Flush。异步虚面默认同步完成（对齐 C# 内存流语义），
/// 真异步子类（FileStream 文件 I/O 池）以 override 覆写。
/// 同步物化：<see cref="MemoryStream"/>、<see cref="FileStream"/>。
/// <see cref="CopyTo"/> 为纯 Arc 默认实现（经 Read/Write 循环）。
/// </summary>
public abstract class Stream : IDisposable {
    /// <summary>流是否支持读取。</summary>
    public abstract bool CanRead { get; }

    /// <summary>流是否支持写入。</summary>
    public abstract bool CanWrite { get; }

    /// <summary>流是否支持定位。</summary>
    public abstract bool CanSeek { get; }

    /// <summary>流长度（字节）。</summary>
    public abstract long Length { get; }

    /// <summary>当前读写位置。</summary>
    public abstract long Position { get; set; }

    /// <summary>读取字节到缓冲区。</summary>
    public abstract int Read(byte[] buffer, int offset, int count);

    /// <summary>将缓冲区字节写入流。</summary>
    public abstract void Write(byte[] buffer, int offset, int count);

    /// <summary>设置流的读写位置。</summary>
    public abstract long Seek(long offset, SeekOrigin origin);

    /// <summary>设置流长度。</summary>
    public abstract void SetLength(long value);

    /// <summary>将缓冲区数据写入底层存储并清空缓冲区。</summary>
    public abstract void Flush();

    /// <summary>
    /// 将本流从当前位置起的剩余字节拷贝到 <paramref name="destination"/>。
    /// 使用固定 64 字节缓冲的 Read/Write 循环（诚实默认真体，非 stub）。
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

    // ── 异步虚面（RFC 038 M2 · 对标 C# Stream.ReadAsync/WriteAsync/FlushAsync）──
    //
    // 默认实现经同步 Read/Write/Flush 同步完成（对齐 C# 内存流语义）；真异步子类
    // （FileStream 文件 I/O 池、NetworkStream 等）以 override 覆写为 Reactor/池卸载真异步。
    // CancellationToken 仅在提交前预检（已取消返回已取消 Task，不执行底层 I/O）。

    /// <summary>
    /// 异步读取至多 <paramref name="count"/> 字节到缓冲区（自 <paramref name="offset"/> 起）；
    /// 返回实际读取字节数；EOF 返回 0。默认实现同步完成（对齐 C# MemoryStream 语义）。
    /// </summary>
    /// <param name="cancellationToken">取消令牌：提交前已取消则任务直接置为已取消。</param>
    public virtual Task<int> ReadAsync(byte[] buffer, int offset, int count, CancellationToken cancellationToken = default) {
        if (cancellationToken.IsCancellationRequested) {
            return Task.FromCanceled<int>(cancellationToken);
        }
        return Task.FromResult(this.Read(buffer, offset, count));
    }

    /// <summary>
    /// 异步全量写入 <paramref name="count"/> 字节（自 <paramref name="offset"/> 起）。
    /// 默认实现同步完成（对齐 C# MemoryStream 语义）。
    /// </summary>
    /// <param name="cancellationToken">取消令牌：提交前已取消则任务直接置为已取消。</param>
    public virtual Task WriteAsync(byte[] buffer, int offset, int count, CancellationToken cancellationToken = default) {
        if (cancellationToken.IsCancellationRequested) {
            return Task.FromCanceled(cancellationToken);
        }
        this.Write(buffer, offset, count);
        return Task.CompletedTask;
    }

    /// <summary>
    /// 异步刷新写入缓冲（对齐 <see cref="Flush"/>）。默认实现同步完成（对齐 C# MemoryStream 语义）。
    /// </summary>
    /// <param name="cancellationToken">取消令牌：提交前已取消则任务直接置为已取消。</param>
    public virtual Task FlushAsync(CancellationToken cancellationToken = default) {
        if (cancellationToken.IsCancellationRequested) {
            return Task.FromCanceled(cancellationToken);
        }
        this.Flush();
        return Task.CompletedTask;
    }

    /// <summary>关闭流并释放资源。</summary>
    public void Close() {
        this.Dispose();
    }

    /// <summary>是否已释放。</summary>
    protected bool _disposed;

    /// <summary>释放流持有的资源。</summary>
    public virtual void Dispose() {
        if (!_disposed) {
            _disposed = true;
        }
    }
}
