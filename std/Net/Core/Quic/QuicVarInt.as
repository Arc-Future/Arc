// S4 (RFC 033 §2.6): Arc.Net.Quic — QUIC varint 编解码（RFC 9000 §16）。
//
// 纯 Arc 实现。语言缺位运算（§0.1 不得倒逼语言洞）：前导位/掩码以除法/取模算术仿真
// （与 std/Net/Core/Http/Http2/Hpack.as 同例）。
//
// 诚实边界：varint 数值域按 RFC 9000 覆盖 62 位；本最小子集实际使用小帧长/小索引，
// 但编解码实现完整支持 1/2/4/8 字节四种形态。

namespace Arc.Net.Quic;

using Arc.Collections;

/// <summary>QUIC varint（RFC 9000 §16）编解码工具。</summary>
public class QuicVarInt {
    /// <summary>值所需编码长度（1/2/4/8 字节）。</summary>
    public static int EncodeLength(long value) {
        if (value < 64) { return 1; }
        if (value < 16384) { return 2; }
        if (value < 1073741824) { return 4; }
        return 8;
    }

    /// <summary>向输出追加一个 varint。value 必须 ≥ 0。</summary>
    public static void Encode(List<byte> out_, long value) {
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
            int i = 0;
            long v = value;
            while (i < 3) {
                v = v / 256;
                out_.Add((byte)(v % 256));
                i = i + 1;
            }
            return;
        }
            out_.Add((byte)(192 + value / 72057594037927936));
        long rest = value;
        int k = 0;
        while (k < 7) {
            rest = rest / 256;
            out_.Add((byte)(rest % 256));
            k = k + 1;
        }
    }

    /// <summary>从 data[offset] 解码一个 varint。成功返回数值并把实际字节长度写入 len；
    /// 数据不足/越界返回 -1。</summary>
    public static long Decode(byte[] data, int offset, out int len) {
        len = 0;
        if (offset >= data.Length) { return -1; }
        int b = data[offset];
        int width = b / 64 + 1; // 1/2/4/8
        if (width == 1) {
            if (offset + 1 > data.Length) { return -1; }
            len = 1;
            return b % 64;
        }
        if (width == 2) {
            if (offset + 2 > data.Length) { return -1; }
            len = 2;
            return (b % 64) * 256 + data[offset + 1];
        }
        if (width == 4) {
            if (offset + 4 > data.Length) { return -1; }
            len = 4;
            long v = b % 64;
            int i = 1;
            while (i < 4) {
                v = v * 256 + data[offset + i];
                i = i + 1;
            }
            return v;
        }
        if (offset + 8 > data.Length) { return -1; }
        len = 8;
        long v8 = b % 64;
        int j = 1;
        while (j < 8) {
            v8 = v8 * 256 + data[offset + j];
            j = j + 1;
        }
        return v8;
    }
}
