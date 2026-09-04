// RFC 033 §1.0: Arc.Net — 内存承载 StreamTransport（供 HttpContent.ReadAsStream 全缓冲回退）。
//
// 对齐 C# MemoryStream 之于 Stream 的定位：把已全缓冲的 byte[] 提升为
// StreamTransport 统一读面，使 HttpContent.ReadAsStream 对「流式（活传输）」
// 与「全缓冲（内存）」返回同构的读载体（单一惯用法）。实现为只读内存载体：
// ReadLine/ReadString/ReadToEnd 均从内部缓冲解码；写面抛异常（只读）。
//
// 语言缺口（RFC 014 已知债）：byte[] 实例字段不支持 `.Length`（TypeId 解析为
// byte_arr）→ 各读法先将字段拷本地再取长度（同 TlsClientSession 惯例）。
namespace Arc.Net;

using Arc.Collections;
using Arc.Text;

/// <summary>
/// 内存承载的只读 StreamTransport——包装已全缓冲 byte[] 为统一读面。
/// 供 HttpContent.ReadAsStream 在无活动传输（全缓冲响应）时返回。
/// </summary>
public class MemoryStreamTransport : StreamTransport {
    private byte[] _buffer;
    private int _pos;

    public MemoryStreamTransport(byte[] buffer) {
        _buffer = buffer != null ? buffer : ZeroBytes(0);
        _pos = 0;
    }

    /// <summary>n 字节零填充数组（语言禁 `new T[expr]` 动态尺寸；同 TlsClientSession 惯例）。</summary>
    private static byte[] ZeroBytes(int n) {
        List<byte> buf = new List<byte>();
        int i = 0;
        while (i < n) {
            buf.Add((byte)0);
            i = i + 1;
        }
        return buf.ToArray();
    }

    /// <summary>内存载体总长度。</summary>
    public int Length {
        get { byte[] buf = _buffer; return buf.Length; }
    }

    public override int Read(byte[] buffer, int offset, int count) {
        byte[] src = _buffer;
        int remaining = src.Length - _pos;
        int take = count < remaining ? count : remaining;
        if (take <= 0) { return 0; }
        int i = 0;
        while (i < take) {
            buffer[offset + i] = src[_pos + i];
            i = i + 1;
        }
        _pos = _pos + take;
        return take;
    }

    public override void Write(byte[] buffer, int offset, int count) {
        throw new NotSupportedException("MemoryStreamTransport is read-only");
    }

    public override string ReadString(int bufferSize) {
        byte[] src = _buffer;
        int remaining = src.Length - _pos;
        int take = bufferSize < remaining ? bufferSize : remaining;
        if (take <= 0) { return null; }
        List<byte> slice = new List<byte>();
        int i = 0;
        while (i < take) {
            slice.Add(src[_pos + i]);
            i = i + 1;
        }
        _pos = _pos + take;
        return Encoding.GetString(slice.ToArray());
    }

    public override int WriteString(string data) {
        throw new NotSupportedException("MemoryStreamTransport is read-only");
    }

    public override string ReadLine() {
        byte[] src = _buffer;
        if (_pos >= src.Length) { return null; }
        int start = _pos;
        int end = _pos;
        while (end < src.Length && !(src[end] == (byte)10)) {
            end = end + 1;
        }
        int lineEnd = end;
        if (lineEnd > start && src[lineEnd - 1] == (byte)13) {
            lineEnd = lineEnd - 1;
        }
        List<byte> line = new List<byte>();
        int i = start;
        while (i < lineEnd) {
            line.Add(src[i]);
            i = i + 1;
        }
        _pos = end < src.Length ? end + 1 : end;
        return Encoding.GetString(line.ToArray());
    }

    public override string ReadToEnd() {
        byte[] src = _buffer;
        if (_pos >= src.Length) { return ""; }
        List<byte> rest = new List<byte>();
        int i = _pos;
        while (i < src.Length) {
            rest.Add(src[i]);
            i = i + 1;
        }
        _pos = src.Length;
        return Encoding.GetString(rest.ToArray());
    }

    public override void Flush() { }

    public override void Close() { }

    // ── 真异步方法面（内存承载 · 瞬时完成，Task.FromResult 包装同步语义）──

    public Task<int> ReadBytesAsync(byte[] buffer, int offset, int count) {
        return Task.FromResult(this.Read(buffer, offset, count));
    }

    public Task WriteBytesAsync(byte[] buffer, int offset, int count) {
        this.Write(buffer, offset, count);
        return Task.FromResult(0);
    }

    public Task<string> ReadStringAsync(int bufferSize) {
        return Task.FromResult(this.ReadString(bufferSize));
    }

    public Task<int> WriteStringAsync(string data) {
        return Task.FromResult(this.WriteString(data));
    }

    public Task<string> ReadLineAsync() {
        return Task.FromResult(this.ReadLine());
    }

    public Task<string> ReadToEndAsync() {
        return Task.FromResult(this.ReadToEnd());
    }
}
