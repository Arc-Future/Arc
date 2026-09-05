// WebTransportVarInt —— 拆分自 WebTransportTypes.as（一文件一公开类型）。
namespace Arc.Net.WebTransport;
using Arc.Collections;

/// <summary>RFC 9000 §16 varint 编解码（WebTransport 命名空间内自包含）。
///
/// 背景：S4 `Arc.Net.Quic.QuicVarInt` 对 4/8 字节形态存在缺陷——Encode 的 4 字节
/// 分支字节序错误（右移步长写反），Decode 的宽度公式 `b / 64 + 1` 对首字节
/// 128..191 得 3、192..255 得 4，误落入 8 字节分支返回 -1。WebTransport 数据面
/// 必需 4 字节 varint（WT_STREAM 等 capsule 类型 0x190B4D3*、SETTINGS_WT_ENABLED
/// 0x2c7cf000、DrainSession 0x78AE），故在本层自包含实现正确编解码；
/// 不改 S4 冻结面，缺陷如实记录于 039 W1/W2 验收注记。</summary>
internal class WebTransportVarInt {
    internal static int EncodeLength(long value) {
        if (value < 64) { return 1; }
        if (value < 16384) { return 2; }
        if (value < 1073741824) { return 4; }
        return 8;
    }

    internal static void Encode(List<byte> out_, long value) {
        if (value < 0) { return; }
        int len = EncodeLength(value);
        if (len == 1) {
            out_.Add((byte)value);
            return;
        }
        if (len == 2) {
            out_.Add((byte)(64 + value / 256));
            out_.Add((byte)(value % 256));
            return;
        }
        if (len == 4) {
            out_.Add((byte)(128 + value / 16777216));
            out_.Add((byte)((value / 65536) % 256));
            out_.Add((byte)((value / 256) % 256));
            out_.Add((byte)(value % 256));
            return;
        }
        out_.Add((byte)(192 + value / 72057594037927936));
        out_.Add((byte)((value / 281474976710656) % 256));
        out_.Add((byte)((value / 1099511627776) % 256));
        out_.Add((byte)((value / 4294967296) % 256));
        out_.Add((byte)((value / 16777216) % 256));
        out_.Add((byte)((value / 65536) % 256));
        out_.Add((byte)((value / 256) % 256));
        out_.Add((byte)(value % 256));
    }

    /// <summary>从 data[offset] 解码一个 varint；失败返回 -1，len 为实际长度。</summary>
    internal static long Decode(byte[] data, int offset, out int len) {
        len = 0;
        if (data == null || offset >= data.Length) { return -1; }
        int b = data[offset];
        if (b < 64) {
            if (offset + 1 > data.Length) { return -1; }
            len = 1;
            return b;
        }
        if (b < 128) {
            if (offset + 2 > data.Length) { return -1; }
            len = 2;
            return (long)(b - 64) * 256 + data[offset + 1];
        }
        if (b < 192) {
            if (offset + 4 > data.Length) { return -1; }
            len = 4;
            long v = b - 128;
            v = v * 256 + data[offset + 1];
            v = v * 256 + data[offset + 2];
            v = v * 256 + data[offset + 3];
            return v;
        }
        if (offset + 8 > data.Length) { return -1; }
        len = 8;
        long v8 = b - 192;
        int j = 1;
        while (j < 8) {
            v8 = v8 * 256 + data[offset + j];
            j = j + 1;
        }
        return v8;
    }
}
