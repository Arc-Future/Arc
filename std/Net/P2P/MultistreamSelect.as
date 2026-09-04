// RFC 042 M1 (M1a): MultistreamSelect — multistream-select/1.0.0 协议协商（真实实现）。
//
// 真实面（对齐 libp2p multistream-select/1.0.0 wire）：
//   - 帧格式：`<varint length><payload>`，payload = 协议标识字符串（含尾部 \n）。
//   - 握手：initiator 发 `/multistream/1.0.0\n`，listener 复回 `/multistream/1.0.0\n`。
//   - 协议选择：initiator 发 `<协议>\n`；listener 复回 `na\n`（不可用）或 `<协议>\n`（选中）。
//   - 基于 `Arc.Net.NetworkStream`（底层 `rt_socket_*` 真实 TCP），byte[] 帧级编解码。
//
// 诚实边界（M1a 切片）：
//   - 帧长度 varint 取**单字节**（协议标识长度 < 128）。libp2p 常用协议标识
//     （/multistream/1.0.0 · /ipfs/id/1.0.0 · /ipfs/ping/1.0.0 等）均 <128 字节；
//     多字节 varint 长度（≥128 协议标识）后置于 M1b/M3 扩展。本切片以「≤127 字节
//     协议标识」为诚实可用面，超界显式报错（不静默截断）。
//   - 避开 `&` 位与（解析器未接线位与表达式，见 RFC 042 §4 R3）：单字节长度
//     免位运算；读/写均不经位掩码。
//   - 底层 NetworkStream 为 string 传输原语（NUL 截断诚实边界）：帧载荷为 ASCII
//     协议标识（无内部 0x00），长度字节 <0x80，故字节保真往返成立；二进制载荷
//     不属于本切片（N3 已过 · 后续流载荷走 yamux 切片）。
namespace Arc.Net.P2P;

using Arc;
using Arc.Net;
using Arc.Collections;
using Arc.Text;

/// <summary>multistream-select/1.0.0 帧（已解码载荷 + 原始字节）。</summary>
public class MsFrame {
    public string Payload;   // 协议标识（含尾部 \n 前的原文，可含 \n）
    public byte[] Raw;       // 原始帧字节（长度前缀 + 载荷）

    public MsFrame(string payload, byte[] raw) {
        Payload = payload;
        Raw = raw;
    }
}

/// <summary>
/// multistream-select/1.0.0 协商器（client + server 双侧）。真实帧级编解码，
/// 基于 NetworkStream 的 byte[] 面（<see cref="Arc.Net.NetworkStream"/>）。
/// </summary>
public class MultistreamSelect {
    /// <summary>multistream-select 自身协议标识。</summary>
    public const string MultistreamId = "/multistream/1.0.0";
    private const int MaxLen = 127;      // M1a 单字节 varint 长度上界（诚实边界，见文件头）

    // ── 帧编解码 ──

    /// <summary>写一帧：单字节 varint 长度 + 载荷。长度超 <see cref="MaxLen"/> 抛 IOException。</summary>
    public static void WriteFrame(NetworkStream stream, string payload) {
        byte[] body = Encoding.GetBytes(payload);
        if (body.Length > MaxLen) {
            throw new IOException("MultistreamSelect.WriteFrame: protocol id exceeds 127-byte single-byte varint boundary");
        }
        byte[] frame = new byte[body.Length + 1];
        frame[0] = (byte)body.Length;      // 单字节 varint（<128）
        int i = 0;
        while (i < body.Length) {
            frame[i + 1] = body[i];
            i = i + 1;
        }
        stream.Write(frame, 0, frame.Length);
    }

    /// <summary>读一帧：单字节 varint 长度 + 载荷。返回已解码 MsFrame；EOF 返回 null。</summary>
    public static MsFrame ReadFrame(NetworkStream stream) {
        byte[] lenBuf = new byte[1];
        int n = stream.Read(lenBuf, 0, 1);
        if (n <= 0) {
            return null;
        }
        int len = (int)lenBuf[0];
        if (len <= 0 || len > MaxLen) {
            return null;   // 非法/超界长度（诚实边界）
        }
        byte[] body = new byte[len];
        int got = ReadExact(stream, body, len);
        if (got != len) {
            return null;   // 载荷不完整
        }
        string payload = Encoding.GetString(body);
        // 组回原始帧（长度前缀 + 载荷）供互操作断言。
        byte[] raw = new byte[len + 1];
        raw[0] = lenBuf[0];
        int j = 0;
        while (j < len) {
            raw[j + 1] = body[j];
            j = j + 1;
        }
        return new MsFrame(payload, raw);
    }

    /// <summary>从流中精确读取 <paramref name="count"/> 字节到 <paramref name="outBytes"/>
    /// （循环补齐部分读）。<c>outBytes</c> 必须至少 <paramref name="count"/> 长。</summary>
    public static int ReadExact(NetworkStream stream, byte[] outBytes, int count) {
        byte[] one = new byte[1];
        int got = 0;
        while (got < count) {
            int r = stream.Read(one, 0, 1);
            if (r <= 0) {
                return got;
            }
            outBytes[got] = one[0];
            got = got + 1;
        }
        return got;
    }

    // ── 客户端（initiator） ──

    /// <summary>客户端握手：发 /multistream/1.0.0，读 listener 复回。成功返回 true。</summary>
    public static bool ClientHandshake(NetworkStream stream) {
        WriteFrame(stream, MultistreamId + "\n");
        MsFrame resp = ReadFrame(stream);
        if (resp == null) {
            return false;
        }
        return resp.Payload == MultistreamId + "\n";
    }

    /// <summary>
    /// 客户端选择单一协议：发 &lt;协议&gt;\n，按 listener 复回判定。
    /// 返回选中协议标识（成功）或 "na"（对端不支持）或 null（协议错误）。
    /// </summary>
    public static string ClientSelect(NetworkStream stream, string protocolId) {
        WriteFrame(stream, protocolId + "\n");
        MsFrame resp = ReadFrame(stream);
        if (resp == null) {
            return null;
        }
        if (resp.Payload == "na\n") {
            return "na";
        }
        if (resp.Payload == protocolId + "\n") {
            return protocolId;
        }
        return null;
    }

    // ── 服务端（listener） ──

    /// <summary>服务端握手：读 initiator 的 /multistream/1.0.0，复回。成功返回 true。</summary>
    public static bool ServerHandshake(NetworkStream stream) {
        MsFrame req = ReadFrame(stream);
        if (req == null) {
            return false;
        }
        if (req.Payload != MultistreamId + "\n") {
            return false;
        }
        WriteFrame(stream, MultistreamId + "\n");
        return true;
    }

    /// <summary>
    /// 服务端处理单次协议选择：读 initiator 请求，若在 <paramref name="supported"/>
    /// 中则复回该协议标识（选中），否则复回 "na"。
    /// 返回选中协议标识（成功）或 "na"（不支持）或 null（协议错误/EOF）。
    /// </summary>
    public static string ServerHandle(NetworkStream stream, List<string> supported) {
        MsFrame req = ReadFrame(stream);
        if (req == null) {
            return null;
        }
        string id = req.Payload;
        if (id.Length > 0 && id.Substring(id.Length - 1, 1) == "\n") {
            id = id.Substring(0, id.Length - 1);
        }
        // 判定：请求协议是否在支持列表。
        bool found = false;
        int i = 0;
        while (i < supported.Count) {
            if (supported[i] == id) {
                found = true;
                break;
            }
            i = i + 1;
        }
        if (found) {
            WriteFrame(stream, id + "\n");
            return id;
        }
        WriteFrame(stream, "na\n");
        return "na";
    }
}
