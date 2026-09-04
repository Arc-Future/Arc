// RFC 049 M-C: Arc.Net.Grpc — gRPC 5 字节消息分帧 codec（1 字节压缩标志 + 4 字节大端长度）。
//
// 对齐 gRPC over HTTP/2 消息帧：每 DATA 帧载荷 = [压缩标志(0=未压缩)][uint32 大端长度][消息字节]。
// 纯 Arc 实现。语言缺位运算（§0.1 不得倒逼语言洞）：大端长度以乘加分解组装
// （对齐 Http2Frame.Encode 先例）；压缩标志以整型判等。
//
// 诚实边界：
//   - 压缩标志非 0（gzip 等）→ 解码失败返回 null（压缩后置，仅透明未压缩帧）。
//   - 首长度字节 ≥0x80（长度 ≥2³¹，int 必溢出）→ 格式错拒绝（解码失败返回 null）。
//   - 单帧长度上限受传输层帧长 ≤16384 约束（RFC 7540；跨帧分片后置，见 GrpcChannel）。
//
// 访问权限：internal（框架内部实现，非开发者面契约——开发者不直接操作分帧）。

namespace Arc.Net.Grpc;

using Arc.Collections;

/// <summary>gRPC 5 字节消息分帧（压缩标志 + 大端长度 + 消息）。框架内部 codec。</summary>
internal class GrpcMessageCodec {
    /// <summary>帧头固定 5 字节（1 压缩标志 + 4 长度）。</summary>
    public const int FrameHeaderSize = 5;

    /// <summary>单条消息 → 完整 gRPC 帧（[0x00][len BE4][消息]）。</summary>
    public static byte[] EncodeFrame(byte[] message) {
        byte[] msg = message;
        if (msg == null) { msg = ZeroBytes(0); }
        int len = msg.Length;
        List<byte> frame = new List<byte>();
        frame.Add((byte)0); // 压缩标志：未压缩
        frame.Add((byte)((len / 16777216) % 256));
        frame.Add((byte)((len / 65536) % 256));
        frame.Add((byte)((len / 256) % 256));
        frame.Add((byte)(len % 256));
        int i = 0;
        while (i < len) {
            frame.Add(msg[i]);
            i = i + 1;
        }
        return frame.ToArray();
    }

    /// <summary>
    /// 从流 <paramref name="stream"/> 的 <paramref name="pos"/> 起解一帧 → 消息字节，
    /// 并推进 <paramref name="nextPos"/>。流结束（不足 5 字节）/ 格式错（压缩非 0 /
    /// 首长度字节 ≥0x80 / 长度越界）返回 null。
    /// </summary>
    public static byte[] ReadFrame(byte[] stream, int pos, out int nextPos) {
        byte[] s = stream;
        int avail = s.Length - pos;
        if (avail < FrameHeaderSize) { nextPos = pos; return null; }
        int comp = (int)s[pos];
        if (comp != 0) { nextPos = pos; return null; } // 压缩后置
        if ((int)s[pos + 1] >= 128) { nextPos = pos; return null; } // 首长度字节 ≥0x80：int 乘加必溢出，格式错拒绝
        int len = ((int)s[pos + 1] * 16777216)
            + ((int)s[pos + 2] * 65536)
            + ((int)s[pos + 3] * 256)
            + (int)s[pos + 4];
        if (len > avail - FrameHeaderSize) { nextPos = pos; return null; }
        List<byte> msg = new List<byte>();
        int i = 0;
        while (i < len) {
            msg.Add(s[pos + FrameHeaderSize + i]);
            i = i + 1;
        }
        nextPos = pos + FrameHeaderSize + len;
        return msg.ToArray();
    }

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
