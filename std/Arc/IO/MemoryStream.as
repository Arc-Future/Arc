// 标准库就绪 P0：MemoryStream — 字节缓冲流（Stable）。
// 对标 C# System.IO.MemoryStream 最小同步面。

namespace Arc.IO;

using Arc.Collections;

/// <summary>
/// 内存字节流——以可扩容缓冲提供 <see cref="Stream"/> 同步读写与定位。
/// </summary>
/// <remarks>
/// Stable：Read / Write / Seek / Position / Length / SetLength / Flush / ToArray / Dispose。
/// CopyTo 继承自 <see cref="Stream"/> 默认真体。异步虚面（ReadAsync/WriteAsync/FlushAsync）
/// 继承 <see cref="Stream"/> 默认同步完成实现（对齐 C# MemoryStream 语义）。
/// </remarks>
public class MemoryStream : Stream {
    private List<byte> _buffer;
    private int _position;
    private bool _isOpen;

    /// <summary>创建可扩容空流。</summary>
    public MemoryStream() {
        _buffer = new List<byte>();
        _isOpen = true;
    }

    /// <summary>以已有字节数组内容初始化（拷贝）。</summary>
    public MemoryStream(byte[] buffer) {
        _buffer = new List<byte>();
        _isOpen = true;
        if (buffer != null) {
            this.Write(buffer, 0, buffer.Length);
            _position = 0;
        }
    }

    public override bool CanRead {
        get { return _isOpen; }
    }

    public override bool CanWrite {
        get { return _isOpen; }
    }

    public override bool CanSeek {
        get { return _isOpen; }
    }

    public override long Length {
        get { return (long)_buffer.Count; }
    }

    public override long Position {
        get { return (long)_position; }
        set {
            _ensureOpen();
            int pos = (int)value;
            if (pos < 0) {
                throw new ArgumentOutOfRangeException("Position");
            }
            _position = pos;
        }
    }

    public override int Read(byte[] buffer, int offset, int count) {
        _ensureOpen();
        if (buffer == null) {
            throw new ArgumentNullException("buffer");
        }
        if (offset < 0 || count < 0 || offset + count > buffer.Length) {
            throw new ArgumentOutOfRangeException("offset/count");
        }
        int avail = _buffer.Count - _position;
        if (avail <= 0) {
            return 0;
        }
        int n = count;
        if (n > avail) {
            n = avail;
        }
        int i = 0;
        while (i < n) {
            buffer[offset + i] = _buffer[_position + i];
            i = i + 1;
        }
        _position = _position + n;
        return n;
    }

    public override void Write(byte[] buffer, int offset, int count) {
        _ensureOpen();
        if (buffer == null) {
            throw new ArgumentNullException("buffer");
        }
        if (offset < 0 || count < 0 || offset + count > buffer.Length) {
            throw new ArgumentOutOfRangeException("offset/count");
        }
        int i = 0;
        while (i < count) {
            if (_position < _buffer.Count) {
                _buffer[_position] = buffer[offset + i];
            } else {
                _buffer.Add(buffer[offset + i]);
            }
            _position = _position + 1;
            i = i + 1;
        }
    }

    public override long Seek(long offset, SeekOrigin origin) {
        _ensureOpen();
        int basePos = 0;
        if (origin == SeekOrigin.Begin) {
            basePos = 0;
        } else if (origin == SeekOrigin.Current) {
            basePos = _position;
        } else {
            basePos = _buffer.Count;
        }
        int next = basePos + (int)offset;
        if (next < 0) {
            throw new IOException("Seek before begin");
        }
        _position = next;
        return (long)_position;
    }

    public override void SetLength(long value) {
        _ensureOpen();
        int target = (int)value;
        if (target < 0) {
            throw new ArgumentOutOfRangeException("value");
        }
        while (_buffer.Count < target) {
            _buffer.Add((byte)0);
        }
        while (_buffer.Count > target) {
            _buffer.RemoveAt(_buffer.Count - 1);
        }
        if (_position > target) {
            _position = target;
        }
    }

    public override void Flush() {
        _ensureOpen();
    }

    /// <summary>将当前缓冲内容拷贝为新 <c>byte[]</c>（不影响 Position）。</summary>
    public byte[] ToArray() {
        return _buffer.ToArray();
    }

    public override void Dispose() {
        if (_isOpen) {
            _isOpen = false;
            _buffer.Clear();
            _position = 0;
        }
    }

    private void _ensureOpen() {
        if (!_isOpen) {
            throw new ObjectDisposedException("MemoryStream");
        }
    }
}
