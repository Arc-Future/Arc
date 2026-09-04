// RFC 025 M4: Arc.Net — NetworkStream 流式抽象层。
//
// 对标 C# System.Net.Sockets.NetworkStream（.NET 9）。
// NetworkStream 包装 TcpClient 提供流式 I/O 操作：
//   - Read(byte[], offset, count) → 读取字节到缓冲区（返回实际字节数；EOF→0）
//   - Write(byte[], offset, count) → 全量写入缓冲区字节
//   - ReadString(bufferSize) → 读取字符串 / WriteString(string) → 写入字符串
//   - ReadLine() → 读取一行（StreamReader 风格）
//   - ReadToEnd() → 读取所有剩余数据
// 纯 Arc 代码（非 facade），基于 TcpClient 构建。
//
// N3（RFC 035 已知债）：byte[] 网络数据缓冲面——`Read(byte[], offset, count)` /
// `Write(byte[], offset, count)` 闭环（读返回实际字节数、EOF→0、部分读语义；写全量、
// 失败抛 IOException）。字符串读面为 `ReadString`（与 byte[] `Read` 分名，避免编译器
// 对 `Read(int)` / `Read(byte[], int, int)` 的 int 实参解析缺陷）。诚实边界：底层
// 传输原语为 NUL 终止 string 面（rt_socket_send/receive），载荷内部含 0x00 时按
// strlen 截断——含内部 NUL 的二进制对传后置。

namespace Arc.Net;

using Arc.Collections;
using Arc.Text;

/// <summary>
/// 网络流——为 TcpClient 提供面向流的 I/O 抽象。
///
/// 适用于大文件下载、协议解析等需要缓冲读取的场景。
/// 内部维护读取缓冲区以优化小块读取。
/// </summary>
public class NetworkStream : StreamTransport {
    private TcpClient _client;
    private string _readBuffer;
    private int _readPos;
    private int _timeout;

    /// <summary>创建 NetworkStream 包装指定 TcpClient。</summary>
    /// <param name="client">已连接的 TcpClient。</param>
    public NetworkStream(TcpClient client) {
        _client = client;
        _readBuffer = "";
        _readPos = 0;
        _timeout = 30000;
    }

    /// <summary>创建 NetworkStream 并设置超时。</summary>
    public NetworkStream(TcpClient client, int timeoutMs) {
        _client = client;
        _client.SetReceiveTimeout(timeoutMs);
        _client.SetSendTimeout(timeoutMs);
        _readBuffer = "";
        _readPos = 0;
        _timeout = timeoutMs;
    }

    // ── byte[] 缓冲面（N3 · RFC 035 已知债） ──

    /// <summary>
    /// 从流中读取字节到缓冲区，返回实际读入字节数；流结束（EOF）返回 0。
    /// 单次调用可能返回少于 <paramref name="count"/> 的字节（TCP 部分读语义）。
    /// 与 <see cref="ReadString"/> 平级（字符串读面沿用既有命名）；byte[] 读面为
    /// C# 对齐的 <c>Read(byte[], int, int)</c> 单一惯用法。
    /// 诚实边界：载荷含内部 0x00 时按 NUL 截断（string 传输原语），返回 0 视为 EOF。
    /// </summary>
    public override int Read(byte[] buffer, int offset, int count) {
        if (buffer == null) {
            throw new ArgumentNullException("buffer");
        }
        if (offset < 0 || count < 0 || offset + count > buffer.Length) {
            throw new ArgumentOutOfRangeException("offset/count");
        }
        if (count == 0) {
            return 0;
        }

        int copied = 0;
        int srcPos = offset;
        int remaining = count;

        // 1. 先消费内部行缓冲的剩余字节（保持 string/byte[] 读面一致）。
        if (_readPos < _readBuffer.Length) {
            string leftover = _readBuffer.Substring(_readPos, _readBuffer.Length - _readPos);
            byte[] lb = Encoding.GetBytes(leftover);
            int take = remaining;
            if (take > lb.Length) {
                take = lb.Length;
            }
            int i = 0;
            while (i < take) {
                buffer[srcPos + i] = lb[i];
                i = i + 1;
            }
            _readPos = _readPos + take;
            if (_readPos >= _readBuffer.Length) {
                _readBuffer = "";
                _readPos = 0;
            }
            copied = take;
            if (copied >= count) {
                return copied;
            }
            srcPos = srcPos + take;
            remaining = remaining - take;
        }

        // 2. 网络补齐剩余（单次 recv，部分读语义；EOF/超时返回已有字节）。
        string chunk = _client.Receive(remaining);
        if (chunk == null || chunk == "") {
            return copied;
        }
        byte[] cb = Encoding.GetBytes(chunk);
        int take2 = remaining;
        if (take2 > cb.Length) {
            take2 = cb.Length;
        }
        int j = 0;
        while (j < take2) {
            buffer[srcPos + j] = cb[j];
            j = j + 1;
        }
        copied = copied + take2;
        return copied;
    }

    /// <summary>
    /// 将 <paramref name="count"/> 字节从 <paramref name="buffer"/>[<paramref name="offset"/>..]
    /// 写入流（全量写；底层发送失败抛 <see cref="IOException"/>）。
    /// 与 <see cref="Write(string)"/> 平级；byte[] 写面为 C# 对齐的
    /// <c>Write(byte[], int, int)</c> 单一惯用法。
    /// 诚实边界：底层传输原语为 NUL 终止 string 面——载荷含内部 0x00 时按 strlen 截断（后置）。
    /// </summary>
    public override void Write(byte[] buffer, int offset, int count) {
        if (buffer == null) {
            throw new ArgumentNullException("buffer");
        }
        if (offset < 0 || count < 0 || offset + count > buffer.Length) {
            throw new ArgumentOutOfRangeException("offset/count");
        }
        if (count == 0) {
            return;
        }
        List<byte> slice = new List<byte>();
        int i = 0;
        while (i < count) {
            slice.Add(buffer[offset + i]);
            i = i + 1;
        }
        byte[] bytes = slice.ToArray();
        string data = Encoding.GetString(bytes);
        int sent = _client.Send(data);
        if (sent != count) {
            throw new IOException("NetworkStream.Write failed: expected " + count.ToString() + " bytes, sent " + sent.ToString());
        }
    }

    // ── 字符串级 I/O ──

    /// <summary>从流中读取数据到字符串缓冲区。</summary>
    /// <param name="bufferSize">期望读取的字节数。</param>
    /// <returns>读取到的字符串；连接关闭或超时返回 null。</returns>
    public override string ReadString(int bufferSize) {
        if (_readPos < _readBuffer.Length) {
            // 缓冲区中有剩余数据
            string leftover = _readBuffer.Substring(_readPos, _readBuffer.Length - _readPos);
            if (leftover.Length >= bufferSize) {
                string result = leftover.Substring(0, bufferSize);
                _readPos = _readPos + bufferSize;
                return result;
            }
            // 缓冲区数据不足，先从网络读取更多
            string netData = _client.Receive();
            if (netData == null || netData == "") {
                // 没有更多数据，返回缓冲区剩余部分
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
            string netData = _client.Receive();
            if (netData == null || netData == "") { return null; }
            _readBuffer = netData;
            _readPos = 0;
        }

        if (_readBuffer == "") { return null; }
        string available = _readBuffer;
        _readBuffer = "";
        _readPos = 0;
        if (available.Length <= bufferSize) { return available; }
        // 返回请求的大小，剩余留在缓冲区
        string chunk = available.Substring(0, bufferSize);
        _readBuffer = available;
        _readPos = bufferSize;
        return chunk;
    }

    /// <summary>向流中写入字符串数据（与 <see cref="ReadString"/> 对称的 string 级写面）。</summary>
    /// <param name="data">待写入的数据。</param>
    /// <returns>实际写入的字节数；失败返回 0。</returns>
    public override int WriteString(string data) {
        return _client.Send(data);
    }

    // ── 异步 I/O（RFC 038 M2） ──

    /// <summary>异步从流中读取数据。基于底层 TcpClient 的 ReceiveAsync。</summary>
    /// <param name="bufferSize">期望读取的字节数。</param>
    /// <returns>表示异步读取操作的 Task&lt;string&gt;；完成后返回读取到的字符串。</returns>
    public Task<string> ReadAsync(int bufferSize) {
        return _client.ReceiveAsync(bufferSize);
    }

    /// <summary>异步从流中读取数据（默认 4096 字节缓冲区）。</summary>
    public Task<string> ReadAsync() {
        return _client.ReceiveAsync();
    }

    /// <summary>异步向流中写入数据。基于底层 TcpClient 的 SendAsync。</summary>
    /// <param name="data">待写入的数据。</param>
    /// <returns>表示异步写入操作的 Task&lt;int&gt;；完成后返回实际写入的字节数。</returns>
    public Task<int> WriteAsync(string data) {
        return _client.SendAsync(data);
    }

    // ── 真异步方法面（RFC 028 异步为主 · 对齐 StreamTransport 契约；Reactor 真异步）──

    /// <summary>异步读取至多 <paramref name="count"/> 字节到缓冲区；返回实际字节数；EOF 返回 0。
    /// 基于 <see cref="TcpClient.ReceiveBytesAsync"/>（Reactor），不阻塞调用线程。</summary>
    public async Task<int> ReadBytesAsync(byte[] buffer, int offset, int count) {
        if (buffer == null) {
            throw new ArgumentNullException("buffer");
        }
        if (offset < 0 || count < 0 || offset + count > buffer.Length) {
            throw new ArgumentOutOfRangeException("offset/count");
        }
        if (count == 0) {
            return 0;
        }
        // 先消费内部行缓冲的剩余字节（与同步 Read 语义一致）。
        int copied = 0;
        int srcPos = offset;
        int remaining = count;
        if (_readPos < _readBuffer.Length) {
            string leftover = _readBuffer.Substring(_readPos, _readBuffer.Length - _readPos);
            byte[] lb = Encoding.GetBytes(leftover);
            int take = remaining < lb.Length ? remaining : lb.Length;
            int i = 0;
            while (i < take) {
                buffer[srcPos + i] = lb[i];
                i = i + 1;
            }
            _readPos = _readPos + take;
            if (_readPos >= _readBuffer.Length) {
                _readBuffer = "";
                _readPos = 0;
            }
            copied = take;
            if (copied >= count) {
                return copied;
            }
            srcPos = srcPos + take;
            remaining = remaining - take;
        }
        // 网络补齐剩余（单次 recv，部分读语义）。
        int n = await _client.ReceiveBytesAsync(buffer, srcPos, remaining);
        if (n <= 0) {
            return copied;
        }
        return copied + n;
    }

    /// <summary>异步全量写入 <paramref name="count"/> 字节；失败抛 IOException。
    /// 基于 <see cref="TcpClient.SendBytesAsync"/>（Reactor），不阻塞调用线程。</summary>
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
        int sent = await _client.SendBytesAsync(buffer, offset, count);
        if (sent != count) {
            throw new IOException("NetworkStream.WriteBytesAsync failed: expected " + count.ToString() + " bytes, sent " + sent.ToString());
        }
    }

    /// <summary>异步读取至多 <paramref name="bufferSize"/> 字节为字符串；EOF 返回 null。
    /// 基于 <see cref="TcpClient.ReceiveAsync"/>（Reactor），不阻塞调用线程。</summary>
    public async Task<string> ReadStringAsync(int bufferSize) {
        if (_readPos < _readBuffer.Length) {
            string leftover = _readBuffer.Substring(_readPos, _readBuffer.Length - _readPos);
            if (leftover.Length >= bufferSize) {
                string result = leftover.Substring(0, bufferSize);
                _readPos = _readPos + bufferSize;
                return result;
            }
            string netData = await _client.ReceiveAsync();
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
            string netData = await _client.ReceiveAsync();
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

    /// <summary>异步全量写入字符串（string 级写面）。基于 <see cref="TcpClient.SendAsync"/>（Reactor）。</summary>
    public Task<int> WriteStringAsync(string data) {
        return _client.SendAsync(data);
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
            string netData = await _client.ReceiveAsync();
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
            string chunk = await _client.ReceiveAsync();
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

    // ── 行级 I/O ──

    /// <summary>从流中读取一行（以 \n 或 \r\n 结尾）。</summary>
    /// <returns>行内容（不含换行符）；流结束返回 null。</returns>
    public override string ReadLine() {
        while (true) {
            // 在缓冲区中查找换行
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
                // 提取行内容
                int len = newline - _readPos;
                // Strip trailing \r
                if (len > 0 && _readBuffer.Substring(newline - 1, 1) == "\r") {
                    len = len - 1;
                }
                string line = _readBuffer.Substring(_readPos, len);
                _readPos = newline + 1;
                return line;
            }

            // 缓冲区中没有换行——从网络读取更多
            string netData = _client.Receive();
            if (netData == null || netData == "") {
                // 流结束——返回缓冲区剩余内容
                if (_readPos < _readBuffer.Length) {
                    string leftover = _readBuffer.Substring(_readPos, _readBuffer.Length - _readPos);
                    _readBuffer = "";
                    _readPos = 0;
                    return leftover;
                }
                return null;
            }

            // 追加到缓冲区
            if (_readPos > 0) {
                _readBuffer = _readBuffer.Substring(_readPos, _readBuffer.Length - _readPos);
                _readPos = 0;
            }
            _readBuffer = _readBuffer + netData;
        }
    }

    /// <summary>从流中读取所有剩余数据直到连接关闭。</summary>
    /// <returns>完整的剩余数据字符串。</returns>
    public override string ReadToEnd() {
        string all = "";
        // 先取缓冲区中的剩余数据
        if (_readPos < _readBuffer.Length) {
            all = _readBuffer.Substring(_readPos, _readBuffer.Length - _readPos);
        }
        _readBuffer = "";
        _readPos = 0;

        // 继续从网络读取
        int retries = 0;
        while (retries < 100) {
            string chunk = _client.Receive();
            if (chunk == null || chunk == "") {
                retries = retries + 1;
                int busy = 0;
                while (busy < 5000) { busy = busy + 1; }
                if (retries > 5) { break; }
            } else {
                all = all + chunk;
                retries = 0;
            }
        }
        return all;
    }

    /// <summary>刷新写入缓冲区和释放读取缓冲区。</summary>
    public override void Flush() {
        _readBuffer = "";
        _readPos = 0;
    }

    // ── 属性 ──

    /// <summary>是否有数据可读。</summary>
    public bool DataAvailable() {
        return _readPos < _readBuffer.Length || _client.Available > 0;
    }

    /// <summary>底层 TcpClient（高级用途）。</summary>
    public TcpClient BaseClient { get { return _client; } }

    // ── 生命周期 ──

    /// <summary>关闭 NetworkStream 和底层 TcpClient。</summary>
    public override void Close() {
        _client.Close();
    }

    /// <summary>释放 NetworkStream 资源。</summary>
    public void Dispose() {
        this.Close();
    }
}
