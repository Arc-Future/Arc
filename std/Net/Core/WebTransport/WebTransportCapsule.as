// RFC 039 W1/W2: Arc.Net.WebTransport — capsule 协议编解码（RFC 9297 · RFC 9298
// §2.2 通用 QUIC capsule 帧格式：Capsule Type 与 Length 均为 QUIC varint）。
//
// W1 数据面全部以 capsule 形态复用在 HTTP/2 extended CONNECT 流上：
//   - DATAGRAM capsule（Type 0x00）承载用户数据报；
//   - WT_STREAM / WT_STREAM_FIN（Type 0x190b4d3c / 0x190b4d3b）承载用户流字节，
//     载荷 = Stream ID varint + 用户字节；
//   - WT_CLOSE_SESSION（0x2843）携带 4 字节关闭码 + 消息字节，完成关闭握手；
//   - WT_MAX_DATA / WT_MAX_STREAM_DATA 为流量控制授权（发送侧须先收到对端授权）。
//
// 语言能力缺口（不改语言）：`List<T[]>` 元素 Add 损坏 → 解析面以 blob + lengths
// 平坦表示承载；`byte[]` 字段直读不支持 .Length/索引 → 先拷贝局部。
namespace Arc.Net.WebTransport;

using Arc.Collections;
using Arc.Text;

/// <summary>capsule 编解码（纯 Arc）。</summary>
internal class WebTransportCapsule {
    /// <summary>type + payload → capsule 线上字节。</summary>
    internal static byte[] Encode(long type, byte[] payload) {
        List<byte> out_ = new List<byte>();
        WebTransportVarInt.Encode(out_, type);
        WebTransportVarInt.Encode(out_, (long)payload.Length);
        int i = 0;
        while (i < payload.Length) {
            out_.Add(payload[i]);
            i = i + 1;
        }
        return out_.ToArray();
    }

    /// <summary>宽容解析 capsule 序列：type/length 分别写入列表，payload 字节追加到
    /// blob（第 i 个 capsule 载荷 = blob[Σlengths[0..i), Σlengths[0..i+1))）。
    /// 遇到不完整 capsule（类型/长度越界）即停；consumed 为已消费字节。</summary>
    internal static void Parse(byte[] data, List<long> types, List<int> lengths, List<byte> blob, out int consumed) {
        consumed = 0;
        if (data == null) { return; }
        int off = 0;
        while (off < data.Length) {
            int len;
            long type = WebTransportVarInt.Decode(data, off, out len);
            if (type < 0) { break; }
            off = off + len;
            long clen = WebTransportVarInt.Decode(data, off, out len);
            if (clen < 0) { break; }
            off = off + len;
            if (clen < 0 || off + clen > (long)data.Length) { break; }
            types.Add(type);
            lengths.Add((int)clen);
            int i = 0;
            while (i < (int)clen) {
                blob.Add(data[off + i]);
                i = i + 1;
            }
            off = off + (int)clen;
        }
        consumed = off;
    }

    /// <summary>DATAGRAM capsule（RFC 9297 §3）。</summary>
    internal static byte[] MakeDatagram(byte[] data) {
        return Encode(WebTransportCapsuleTypes.Datagram, data);
    }

    /// <summary>WT_STREAM / WT_STREAM_FIN capsule（draft-ietf-webtrans-http2-15 §4）。</summary>
    internal static byte[] MakeStream(bool fin, long streamId, byte[] data) {
        List<byte> payload = new List<byte>();
        WebTransportVarInt.Encode(payload, streamId);
        int i = 0;
        while (i < data.Length) {
            payload.Add(data[i]);
            i = i + 1;
        }
        long type = WebTransportCapsuleTypes.Stream;
        if (fin) {
            type = WebTransportCapsuleTypes.StreamFin;
        }
        return Encode(type, payload.ToArray());
    }

    /// <summary>WT_MAX_DATA capsule（全局数据量授权）。</summary>
    internal static byte[] MakeMaxData(long maxData) {
        List<byte> payload = new List<byte>();
        WebTransportVarInt.Encode(payload, maxData);
        return Encode(WebTransportCapsuleTypes.MaxData, payload.ToArray());
    }

    /// <summary>WT_MAX_STREAM_DATA capsule（单流数据量授权）。</summary>
    internal static byte[] MakeMaxStreamData(long streamId, long maxStreamData) {
        List<byte> payload = new List<byte>();
        WebTransportVarInt.Encode(payload, streamId);
        WebTransportVarInt.Encode(payload, maxStreamData);
        return Encode(WebTransportCapsuleTypes.MaxStreamData, payload.ToArray());
    }

    /// <summary>WT_CLOSE_SESSION capsule（4 字节关闭码 + UTF-8 消息）。</summary>
    internal static byte[] MakeCloseSession(int code, string message) {
        List<byte> payload = new List<byte>();
        payload.Add((byte)((code / 16777216) % 256));
        payload.Add((byte)((code / 65536) % 256));
        payload.Add((byte)((code / 256) % 256));
        payload.Add((byte)(code % 256));
        byte[] mb = Encoding.GetBytes(message);
        int i = 0;
        while (i < mb.Length) {
            payload.Add(mb[i]);
            i = i + 1;
        }
        return Encode(WebTransportCapsuleTypes.CloseSession, payload.ToArray());
    }

    /// <summary>解析 WT_CLOSE_SESSION payload：code 为 4 字节大端关闭码，message
    /// 为剩余字节 UTF-8 解码。</summary>
    internal static bool ParseCloseSessionPayload(byte[] payload, out int code, out string message) {
        code = 0;
        message = "";
        if (payload == null || payload.Length < 4) { return false; }
        byte[] pl = payload;
        code = (pl[0] * 16777216) + (pl[1] * 65536) + (pl[2] * 256) + pl[3];
        if (pl.Length > 4) {
            int n = pl.Length - 4;
            List<byte> msg = new List<byte>();
            int i = 0;
            while (i < n) {
                msg.Add(pl[4 + i]);
                i = i + 1;
            }
            byte[] mb = msg.ToArray();
            message = Encoding.GetString(mb);
        }
        return true;
    }
}
