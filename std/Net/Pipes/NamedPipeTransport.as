// RFC 048 §4: Arc.Net.Pipes — NamedPipeTransport string 面适配器（M1）。
//
// 聚合字节流门面（NamedPipeServerStream / NamedPipeClientStream），桥接
// Arc.Net 的 string 契约消费方（对标 StreamTransport 的 string 面：
// ReadLine / WriteString / ReadToEnd）。
//
// M1 形态约束（诚实边界）：持有**具体门面类型**并直调——模式 A 门面（对象即
// 裸 RtPipe*，无 ArcHeader）**不可经抽象基类引用虚调用**（vtable 槽读取落空，
// RFC 006「基类引用存储」语言缺口的同型面），字段化基类引用会 0xC0000005。
// 亦因此不继承 StreamTransport（其 async 抽象面需 pipe 真异步 ABI，M2；
// 同步伪装异步违反 RFC 028）。M2 随 Reactor 面升级继承并重审基类引用形态。
//
// 行协议：ReadLine 组装至 \n（剥离尾部 \r\n / \n），EOF 返回 null；
// WriteString 经 Arc.Text.Encoding.GetBytes 编码后全量写入。

namespace Arc.Net.Pipes;

using Arc;
using Arc.IO;
using Arc.Text;

/// <summary>
/// 命名管道 string 面适配器——在管道字节契约上提供行/串读写。
/// </summary>
public class NamedPipeTransport {
    private NamedPipeServerStream _server;
    private NamedPipeClientStream _client;
    private byte[] _lineBuffer;
    private int _bufFilled;
    private int _bufPos;

    /// <summary>包装服务端流（接管其生命周期；Close 转发 Dispose）。</summary>
    public NamedPipeTransport(NamedPipeServerStream server) {
        if (server == null) {
            throw new ArgumentNullException("server");
        }
        _server = server;
        _client = null;
        _lineBuffer = new byte[4096];
        _bufFilled = 0;
        _bufPos = 0;
    }

    /// <summary>包装客户端流（接管其生命周期；Close 转发 Dispose）。</summary>
    public NamedPipeTransport(NamedPipeClientStream client) {
        if (client == null) {
            throw new ArgumentNullException("client");
        }
        _server = null;
        _client = client;
        _lineBuffer = new byte[4096];
        _bufFilled = 0;
        _bufPos = 0;
    }

    /// <summary>全量写入字符串（UTF-8 编码）。</summary>
    /// <returns>写入字节数；0 = 对端已关闭。</returns>
    public int WriteString(string data) {
        if (data == null || data.Length == 0) {
            return 0;
        }
        byte[] payload = Encoding.GetBytes(data);
        if (payload.Length == 0) {
            return 0;
        }
        if (_server != null) {
            _server.Write(payload, 0, payload.Length);
        } else {
            _client.Write(payload, 0, payload.Length);
        }
        return payload.Length;
    }

    /// <summary>写入一行（追加 '\n' 行终止符）。</summary>
    /// <returns>写入字节数（含终止符）；0 = 对端已关闭。</returns>
    public int WriteLine(string data) {
        string line = data == null ? "\n" : data + "\n";
        return this.WriteString(line);
    }

    /// <summary>读取一行（至 '\n'，剥离尾部 '\r'）；EOF（对端关闭且无残留）返回 null。
    /// 游标式缓冲：单次 Read 收到的多行残留跨调用保留（补读前先压缩消费位）。</summary>
    public string ReadLine() {
        for (;;) {
            for (int i = _bufPos; i < _bufFilled; i++) {
                if (_lineBuffer[i] == (byte)'\n') {
                    string line = this.AssembleLine(_bufPos, i);
                    _bufPos = i + 1;
                    if (_bufPos >= _bufFilled) {
                        _bufPos = 0;
                        _bufFilled = 0;
                    }
                    return line;
                }
            }
            if (_bufPos > 0) {
                for (int i = _bufPos; i < _bufFilled; i++) {
                    _lineBuffer[i - _bufPos] = _lineBuffer[i];
                }
                _bufFilled -= _bufPos;
                _bufPos = 0;
            }
            if (_bufFilled == _lineBuffer.Length) {
                break;
            }
            int n = this.ReadBytes(_lineBuffer, _bufFilled, _lineBuffer.Length - _bufFilled);
            if (n <= 0) {
                if (_bufFilled == _bufPos) {
                    _bufPos = 0;
                    _bufFilled = 0;
                    return null;
                }
                break;
            }
            _bufFilled += n;
        }
        string rest = this.AssembleLine(_bufPos, _bufFilled);
        _bufPos = 0;
        _bufFilled = 0;
        return rest;
    }

    /// <summary>字节面读取（直调具体门面；0 = 对端有序关闭）。</summary>
    private int ReadBytes(byte[] buffer, int offset, int count) {
        if (_server != null) {
            return _server.Read(buffer, offset, count);
        }
        return _client.Read(buffer, offset, count);
    }

    /// <summary>由缓冲 [start, end) 字节组装行字符串（剥离尾部 '\r'）。</summary>
    private string AssembleLine(int start, int end) {
        while (end > start && (_lineBuffer[end - 1] == (byte)'\r' || _lineBuffer[end - 1] == (byte)'\n')) {
            end--;
        }
        byte[] line = new byte[end - start];
        for (int i = 0; i < end - start; i++) {
            line[i] = _lineBuffer[start + i];
        }
        return Encoding.GetString(line);
    }

    /// <summary>读取全部剩余数据直到对端关闭；EOF 且无数据返回 null。</summary>
    public string ReadToEnd() {
        byte[] acc = new byte[4096];
        int filled = 0;
        for (;;) {
            if (filled == acc.Length) {
                byte[] grown = new byte[acc.Length * 2];
                for (int i = 0; i < filled; i++) {
                    grown[i] = acc[i];
                }
                acc = grown;
            }
            int n = this.ReadBytes(acc, filled, acc.Length - filled);
            if (n <= 0) {
                break;
            }
            filled += n;
        }
        if (filled == 0) {
            return null;
        }
        byte[] body = new byte[filled];
        for (int i = 0; i < filled; i++) {
            body[i] = acc[i];
        }
        return Encoding.GetString(body);
    }

    /// <summary>关闭底层流（转发 Dispose → Terminate；幂等由 runtime closed 守卫保证）。</summary>
    public void Close() {
        if (_server != null) {
            _server.Dispose();
        } else {
            _client.Dispose();
        }
    }
}
