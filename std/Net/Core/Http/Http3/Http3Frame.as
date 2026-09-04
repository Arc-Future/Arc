// S4 (RFC 033 §2.6): Arc.Net — HTTP/3 帧层（RFC 9114 §7.1）。
//
// 纯 Arc 实现。帧形态 `[type varint][length varint][payload]`；varint 编解码
// 复用 Arc.Net.Quic.QuicVarInt（RFC 9000 §16）。
//
// 诚实边界：帧长度上限 16384（与 HTTP/2 对齐，RFC 9114 §7.2.4 默认无上限但
// 本最小子集钳制）；流送达任意分片容忍（ParseFrames 遇不完整帧即停）。

namespace Arc.Net;

using Arc.Collections;
using Arc.Net.Quic;

/// <summary>HTTP/3 帧编解码（RFC 9114 §7.1）。</summary>
internal class Http3Frame {
    /// <summary>组装一帧（type + length + payload 均按 varint）。</summary>
    internal static byte[] Make(long type, byte[] payload) {
        List<byte> out_ = new List<byte>();
        QuicVarInt.Encode(out_, type);
        QuicVarInt.Encode(out_, (long)payload.Length);
        int i = 0;
        while (i < payload.Length) {
            out_.Add(payload[i]);
            i = i + 1;
        }
        return out_.ToArray();
    }

    /// <summary>SETTINGS 帧（RFC 9114 §7.2.4；ids/values 等长成对）。</summary>
    internal static byte[] MakeSettings(List<long> ids, List<long> values) {
        List<byte> payload = new List<byte>();
        int i = 0;
        while (i < ids.Count) {
            QuicVarInt.Encode(payload, ids[i]);
            QuicVarInt.Encode(payload, values[i]);
            i = i + 1;
        }
        return Make(Http3FrameTypes.Settings, payload.ToArray());
    }

    /// <summary>GOAWAY 帧（RFC 9114 §7.2.6）。</summary>
    internal static byte[] MakeGoAway(long streamId) {
        List<byte> payload = new List<byte>();
        QuicVarInt.Encode(payload, streamId);
        return Make(Http3FrameTypes.GoAway, payload.ToArray());
    }

    /// <summary>解析 SETTINGS 载荷（§7.2.4：varint id + varint value 对）。失败返回 false。</summary>
    internal static bool ParseSettings(byte[] payload, List<long> ids, List<long> values) {
        int off = 0;
        while (off < payload.Length) {
            int len;
            long id = QuicVarInt.Decode(payload, off, out len);
            if (id < 0) { return false; }
            off = off + len;
            long value = QuicVarInt.Decode(payload, off, out len);
            if (value < 0) { return false; }
            off = off + len;
            ids.Add(id);
            values.Add(value);
        }
        return true;
    }

    /// <summary>
    /// 容忍解析流上的 HTTP/3 帧序列。类型/载荷长写入 types/lengths，各帧载荷
    /// 字节连续追加进 blob（第 i 帧载荷 = blob[Σlengths[0..i), Σlengths[0..i+1))）；
    /// consumed 给出已消费字节数。遇到不完整帧即停，剩余字节留待下次累积
    /// （QUIC 流可能按任意边界送达）。
    ///
    /// 语言能力缺口：`List<T[]>`（数组泛型元素）在当前编译器下 Add 会损坏元素
    /// （Typeck 归约为 Named("..._arr")，List 元素槽尺寸错配），故以 blob+lengths
    /// 平坦表示规避（与本层注释一致，不改语言）。
    /// </summary>
    internal static void ParseFrames(
        byte[] data,
        List<long> types,
        List<int> lengths,
        List<byte> blob,
        out int consumed)
    {
        consumed = 0;
        int off = 0;
        int len;
        while (off < data.Length) {
            long type = QuicVarInt.Decode(data, off, out len);
            if (type < 0) { break; }
            off = off + len;
            long plen = QuicVarInt.Decode(data, off, out len);
            if (plen < 0) { break; }
            off = off + len;
            if (plen > 16384) { break; } // 超上限：视为不完整/越界，停
            if (off + plen > data.Length) { break; }
            types.Add(type);
            lengths.Add((int)plen);
            int i = 0;
            while (i < (int)plen) {
                blob.Add(data[off + i]);
                i = i + 1;
            }
            off = off + (int)plen;
        }
        consumed = off;
    }
}
