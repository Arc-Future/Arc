// Arc.Diagnostics.ProcessStream — 管道字节流，包装子进程 pipe fd。

namespace Arc.Diagnostics;

using Arc;
using Arc.IO;

/// <summary>
/// 管道字节流——包装子进程的 stdin/stdout/stderr 管道 fd。
/// 字节级 Read/Write 经单字节 C ABI 循环；行级 ReadLine/WriteLine 经 C 行级 ABI。
/// </summary>
public class ProcessStream : Stream {
    private int _fd;
    private bool _canRead;
    private bool _canWrite;

    public ProcessStream(int fd, bool canRead, bool canWrite) {
        _fd = fd;
        _canRead = canRead;
        _canWrite = canWrite;
    }

    public override bool CanRead { get { return _canRead; } }
    public override bool CanWrite { get { return _canWrite; } }
    public override bool CanSeek { get { return false; } }

    public override long Length {
        get { throw new NotSupportedException("ProcessStream does not support Length"); }
    }

    public override long Position {
        get { throw new NotSupportedException("ProcessStream does not support Position"); }
        set { throw new NotSupportedException("ProcessStream does not support Position"); }
    }

    // std P2 契约修复：尽力而为语义（RFC 021 Stream 契约）——返回实际读入
    // [0..count]（部分读），0 = EOF；替代旧实现的「凑满 count 才返回」阻塞循环。
    public override int Read(byte[] buffer, int offset, int count) {
        if (buffer == null) { throw new ArgumentNullException("buffer"); }
        if (offset < 0 || count < 0 || offset + count > buffer.Length) {
            throw new ArgumentOutOfRangeException("offset/count");
        }
        if (count == 0) { return 0; }
        return rt_process.rt_proc_pipe_read(_fd, buffer, offset, count);
    }

    // std P2 效率修复：单次批量系统调用 + 短写补写循环，替代逐字节 FFI。
    // std P3 契约补完：写失败（C 侧 -1 = 系统调用失败）抛 IOException，对齐
    // RFC 021「写失败抛 IOException」——撤销此前登记的静默返回偏离。
    // （count == 0 为合法 no-op；C 侧对 fd<0/count<=0 返回 0 不会在此出现——
    // 循环前置 written < count 保证每次调用 count > 0，n == 0 仅见于无进展，
    // 同样不可静默吞掉否则死循环。）
    public override void Write(byte[] buffer, int offset, int count) {
        if (buffer == null) { throw new ArgumentNullException("buffer"); }
        if (offset < 0 || count < 0 || offset + count > buffer.Length) {
            throw new ArgumentOutOfRangeException("offset/count");
        }
        int written = 0;
        while (written < count) {
            int n = rt_process.rt_proc_pipe_write(_fd, buffer, offset + written, count - written);
            if (n <= 0) {
                throw new IOException("ProcessStream.Write: pipe write failed (written=" + written + "/" + count + ")");
            }
            written = written + n;
        }
    }

    public override long Seek(long offset, SeekOrigin origin) {
        throw new NotSupportedException("ProcessStream does not support Seek");
    }

    public override void SetLength(long value) {
        throw new NotSupportedException("ProcessStream does not support SetLength");
    }

    public override void Flush() {
        /* 管道自动刷新，无需操作 */
    }

    public string? ReadLine() {
        return rt_process.rt_proc_pipe_read_line(_fd);
    }

    public void WriteLine(string line) {
        rt_process.rt_proc_pipe_write_line(_fd, line);
    }

    public void WriteString(string data) {
        rt_process.rt_proc_pipe_write_string(_fd, data);
    }

    // 注：ReadLineAsync 直接调用 FFI（rt_proc_pipe_read_line）而非 this.ReadLine()，
    // 因为方法调用的返回类型经 registry 解析后丢失 nullable 信息（mangle 失配：
    // expected string? found Nullable_string）。FFI 调用经 param_sig_to_type_id
    // 直接返回 Nullable<string>，与 Task<string?> 的 task_inner 匹配。
    public async Task<string?> ReadLineAsync(CancellationToken cancellationToken = default) {
        cancellationToken.ThrowIfCancellationRequested();
        return rt_process.rt_proc_pipe_read_line(_fd);
    }

    public async Task WriteLineAsync(string line, CancellationToken cancellationToken = default) {
        cancellationToken.ThrowIfCancellationRequested();
        this.WriteLine(line);
    }

    public async Task WriteStringAsync(string data, CancellationToken cancellationToken = default) {
        cancellationToken.ThrowIfCancellationRequested();
        this.WriteString(data);
    }

    public override void Dispose() {
        if (_fd >= 0) {
            rt_process.rt_proc_pipe_close(_fd);
            _fd = -1;
        }
    }
}
