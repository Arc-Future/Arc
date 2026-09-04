// RFC 025 M4 / RFC 035 S5: TlsNetworkStream — TLS 明文面流式传输载体。
//
// 对标 C# System.Net.Security.SslStream 之上的流式抽象（去糟粕）：将
// TlsClientSession 的 byte[] 加密读写面（Read/Write(byte[],offset,count)）
// 提升为与 NetworkStream 同构的 string 级 I/O 面（ReadLine / ReadString /
// ReadToEnd / WriteString），使既有 HTTP/SSE 解析（ChunkedStreamReader /
// AI.DeepSeek 等）可对同一套解析逻辑透明切换明文（NetworkStream）与 TLS
// （TlsNetworkStream）传输——深层 https 直连复用既有代码而不改写。
//
// 设计对齐 StreamTransport 契约（基类在上 Arc.Net / 派生在下）与
// NetworkStream 缓冲惯例（内部行缓冲 _readBuffer/_readPos，跨 chunk 拼接）：
// - byte[] 面直接代理到 TlsClientSession.Read/Write（N3 · 显式长度闭环）。
// - string 面经 TLS 明文读拉取 chunk 后按 NetworkStream 语义缓冲/换行解析。
// 诚实边界同 NetworkStream：底层 TLS 明文原语为 byte[]，string 面按 UTF-8
// 解码；载荷含内部 0x00 时 string 面截断（HTTP 头部/SSE 事件面无此问题）。

namespace Arc.Net.Security;

using Arc.Collections;
using Arc.Text;
using Arc.Net;
using Arc.Security.Cryptography;

/// <summary>
/// TLS 明文面网络流——将 <see cref="TlsClientSession"/> 的加密字节流包装为
/// 与 <see cref="NetworkStream"/> 同构的流式传输载体（继承
/// <see cref="StreamTransport"/> 契约）。用于 https 直连：先以 TcpClient 建立
/// TCP，再经 TlsClientSession 完成 TLS 1.3 握手，最后由本类承载 HTTP/SSE 解析。
/// </summary>
public class TlsNetworkStream : StreamTransport {
    private TlsClientSession _tls;
    private string _readBuffer;
    private int _readPos;

    /// <summary>包装已握手（或待握手）的 TLS 客户端会话。</summary>
    /// <param name="tls">已完成 <c>AuthenticateAsClientAsync()</c> 的 TLS 会话。</param>
    public TlsNetworkStream(TlsClientSession tls) {
        if (tls == null) {
            throw new ArgumentNullException("tls");
        }
        _tls = tls;
        _readBuffer = "";
        _readPos = 0;
    }

    // ── byte[] 缓冲面（N3 · 直接代理 TLS 明文读写） ──

    /// <summary>解密明文读（byte[] 面）。返回实际字节数；EOF 返回 0；语义对齐 NetworkStream.Read。</summary>
    public override int Read(byte[] buffer, int offset, int count) {
        return _tls.Read(buffer, offset, count);
    }

    /// <summary>明文写 → 加密发送（全量；失败抛 IOException）。</summary>
    public override void Write(byte[] buffer, int offset, int count) {
        _tls.Write(buffer, offset, count);
    }

    // ── 字符串级 I/O（NetworkStream 缓冲惯例 over TLS 明文读） ──

    /// <summary>从 TLS 明文面读取至多 <paramref name="bufferSize"/> 字符的字符串；EOF 返回 null。</summary>
    public override string ReadString(int bufferSize) {
        if (_readPos < _readBuffer.Length) {
            string leftover = _readBuffer.Substring(_readPos, _readBuffer.Length - _readPos);
            if (leftover.Length >= bufferSize) {
                string result = leftover.Substring(0, bufferSize);
                _readPos = _readPos + bufferSize;
                return result;
            }
            string netData = this.ReceiveString();
            if (netData == null || netData == "") {
                if (leftover != "") {
                    _readBuffer = "";
                    _readPos = 0;
                    return leftover;
                }
                return null;
            }
            _readBuffer = leftover + netData;
            _readPos = 0;
        } else {
            string netData = this.ReceiveString();
            if (netData == null || netData == "") {
                return null;
            }
            _readBuffer = netData;
            _readPos = 0;
        }

        if (_readBuffer == "") {
            return null;
        }
        string available = _readBuffer;
        _readBuffer = "";
        _readPos = 0;
        if (available.Length <= bufferSize) {
            return available;
        }
        string chunk = available.Substring(0, bufferSize);
        _readBuffer = available;
        _readPos = bufferSize;
        return chunk;
    }

    /// <summary>向 TLS 明文面全量写入字符串（string 级写面，与 <see cref="ReadString"/> 对称）。</summary>
    public override int WriteString(string data) {
        if (data == null) {
            return 0;
        }
        byte[] bytes = Encoding.GetBytes(data);
        _tls.Write(bytes, 0, bytes.Length);
        return bytes.Length;
    }

    /// <summary>读取一行（至 \n，剥离尾部 \r）；EOF 返回 null。</summary>
    public override string ReadLine() {
        while (true) {
            int newline = -1;
            int i = _readPos;
            while (i < _readBuffer.Length) {
                if (_readBuffer.Substring(i, 1) == "\n") {
                    newline = i;
                    break;
                }
                i = i + 1;
            }
            if (newline >= 0) {
                int len = newline - _readPos;
                if (len > 0 && _readBuffer.Substring(newline - 1, 1) == "\r") {
                    len = len - 1;
                }
                string line = _readBuffer.Substring(_readPos, len);
                _readPos = newline + 1;
                return line;
            }

            string netData = this.ReceiveString();
            if (netData == null || netData == "") {
                if (_readPos < _readBuffer.Length) {
                    string leftover = _readBuffer.Substring(_readPos, _readBuffer.Length - _readPos);
                    _readBuffer = "";
                    _readPos = 0;
                    return leftover;
                }
                return null;
            }
            if (_readPos > 0) {
                _readBuffer = _readBuffer.Substring(_readPos, _readBuffer.Length - _readPos);
                _readPos = 0;
            }
            _readBuffer = _readBuffer + netData;
        }
    }

    /// <summary>读取全部剩余数据直到连接关闭。</summary>
    public override string ReadToEnd() {
        string all = "";
        if (_readPos < _readBuffer.Length) {
            all = _readBuffer.Substring(_readPos, _readBuffer.Length - _readPos);
        }
        _readBuffer = "";
        _readPos = 0;

        int retries = 0;
        while (retries < 100) {
            string chunk = this.ReceiveString();
            if (chunk == null || chunk == "") {
                retries = retries + 1;
                if (retries > 5) {
                    break;
                }
            } else {
                all = all + chunk;
                retries = 0;
            }
        }
        return all;
    }

    /// <summary>刷新内部读取缓冲。</summary>
    public override void Flush() {
        _readBuffer = "";
        _readPos = 0;
    }

    // ── 真异步方法面（RFC 009 异步为主 · 对齐 StreamTransport 契约；
    //    TLS 明文面经 TlsClientSession.ReadAsync/WriteAsync · Reactor 真异步）──

    /// <summary>异步解密明文读（byte[] 面）。返回实际字节数；EOF 返回 0。</summary>
    public Task<int> ReadBytesAsync(byte[] buffer, int offset, int count) {
        return _tls.ReadAsync(buffer, offset, count);
    }

    /// <summary>异步明文写 → 加密发送（全量；失败抛 IOException）。</summary>
    public async Task WriteBytesAsync(byte[] buffer, int offset, int count) {
        if (buffer == null) {
            throw new ArgumentNullException("buffer");
        }
        if (offset < 0 || count < 0 || offset + count > buffer.Length) {
            throw new ArgumentOutOfRangeException("offset/count");
        }
        if (count == 0) {
            return;
        }
        byte[] slice = ZeroBytes(count);
        int i = 0;
        while (i < count) {
            slice[i] = buffer[offset + i];
            i = i + 1;
        }
        await _tls.WriteAsync(slice, 0, count);
    }

    /// <summary>异步从 TLS 明文面读取至多 <paramref name="bufferSize"/> 字符的字符串；EOF 返回 null。</summary>
    public async Task<string> ReadStringAsync(int bufferSize) {
        if (_readPos < _readBuffer.Length) {
            string leftover = _readBuffer.Substring(_readPos, _readBuffer.Length - _readPos);
            if (leftover.Length >= bufferSize) {
                string result = leftover.Substring(0, bufferSize);
                _readPos = _readPos + bufferSize;
                return result;
            }
            string netData = await this.ReceiveStringAsync();
            if (netData == null || netData == "") {
                if (leftover != "") {
                    _readBuffer = "";
                    _readPos = 0;
                    return leftover;
                }
                return null;
            }
            _readBuffer = leftover + netData;
            _readPos = 0;
        } else {
            string netData = await this.ReceiveStringAsync();
            if (netData == null || netData == "") {
                return null;
            }
            _readBuffer = netData;
            _readPos = 0;
        }
        if (_readBuffer == "") {
            return null;
        }
        string available = _readBuffer;
        _readBuffer = "";
        _readPos = 0;
        if (available.Length <= bufferSize) {
            return available;
        }
        string chunk = available.Substring(0, bufferSize);
        _readBuffer = available;
        _readPos = bufferSize;
        return chunk;
    }

    /// <summary>异步向 TLS 明文面全量写入字符串（string 级写面）。</summary>
    public async Task<int> WriteStringAsync(string data) {
        if (data == null) {
            return 0;
        }
        byte[] bytes = Encoding.GetBytes(data);
        await _tls.WriteAsync(bytes, 0, bytes.Length);
        return bytes.Length;
    }

    /// <summary>异步读取一行（至 \n，剥离尾部 \r）；EOF 返回 null。</summary>
    public async Task<string> ReadLineAsync() {
        while (true) {
            int newline = -1;
            int i = _readPos;
            while (i < _readBuffer.Length) {
                if (_readBuffer.Substring(i, 1) == "\n") {
                    newline = i;
                    break;
                }
                i = i + 1;
            }
            if (newline >= 0) {
                int len = newline - _readPos;
                if (len > 0 && _readBuffer.Substring(newline - 1, 1) == "\r") {
                    len = len - 1;
                }
                string line = _readBuffer.Substring(_readPos, len);
                _readPos = newline + 1;
                return line;
            }
            string netData = await this.ReceiveStringAsync();
            if (netData == null || netData == "") {
                if (_readPos < _readBuffer.Length) {
                    string leftover = _readBuffer.Substring(_readPos, _readBuffer.Length - _readPos);
                    _readBuffer = "";
                    _readPos = 0;
                    return leftover;
                }
                return null;
            }
            if (_readPos > 0) {
                _readBuffer = _readBuffer.Substring(_readPos, _readBuffer.Length - _readPos);
                _readPos = 0;
            }
            _readBuffer = _readBuffer + netData;
        }
    }

    /// <summary>异步读取全部剩余数据直到连接关闭。</summary>
    public async Task<string> ReadToEndAsync() {
        string all = "";
        if (_readPos < _readBuffer.Length) {
            all = _readBuffer.Substring(_readPos, _readBuffer.Length - _readPos);
        }
        _readBuffer = "";
        _readPos = 0;
        int retries = 0;
        while (retries < 100) {
            string chunk = await this.ReceiveStringAsync();
            if (chunk == null || chunk == "") {
                retries = retries + 1;
                if (retries > 5) {
                    break;
                }
            } else {
                all = all + chunk;
                retries = 0;
            }
        }
        return all;
    }

    /// <summary>关闭底层 TLS 会话与传输。</summary>
    public override void Close() {
        _tls.Dispose();
    }

    // ── 属性 ──

    /// <summary>底层 TlsClientSession（高级用途：证书/ALPN/会话恢复等）。</summary>
    public TlsClientSession BaseSession {
        get { return _tls; }
    }

    // ── 私有：TLS 明文读 → string chunk ──

    /// <summary>经 TlsClientSession 明文读拉取至多 4096 字节并解码为字符串；EOF 返回 null。</summary>
    private string ReceiveString() {
        byte[] buf = ZeroBytes(4096);
        int n = _tls.Read(buf, 0, 4096);
        if (n <= 0) {
            return null;
        }
        List<byte> slice = new List<byte>();
        int i = 0;
        while (i < n) {
            slice.Add(buf[i]);
            i = i + 1;
        }
        return Encoding.GetString(slice.ToArray());
    }

    /// <summary>经 <see cref="TlsClientSession.ReadAsync"/> 异步明文读拉取至多 4096 字节并解码为字符串；EOF 返回 null。</summary>
    private async Task<string> ReceiveStringAsync() {
        byte[] buf = ZeroBytes(4096);
        int n = await _tls.ReadAsync(buf, 0, 4096);
        if (n <= 0) {
            return null;
        }
        List<byte> slice = new List<byte>();
        int i = 0;
        while (i < n) {
            slice.Add(buf[i]);
            i = i + 1;
        }
        return Encoding.GetString(slice.ToArray());
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
}