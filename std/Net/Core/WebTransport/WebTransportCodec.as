// RFC 039 W1/W2: Arc.Net.WebTransport — 字节面编解码门面。
//
// 设计决策（与 039 W1/W2 验收注记对齐）：
//   - W1 的 HTTP/2 extended CONNECT 头块、W2 的 HTTP/3 extended CONNECT 头块、
//     W2 客户端控制流（SETTINGS 含 ENABLE_CONNECT_PROTOCOL + SETTINGS_H3_DATAGRAM
//     + 草案版 SETTINGS_WT_ENABLED）、W2 流/数据报映射均为纯 Arc 字节面逻辑，全部
//     集中在本层；传输承载（TCP/h2c 与 QUIC/ngtcp2）由 e2e Rust harness 提供。
//   - W2 传输绑定缺口（诚实边界）：`https://` 在 Arc 侧暂不启动 QUIC 传输
//     （编译器 .ani 契约面限制，rt_quic_* 由 harness 直调），ConnectAsync 对
//     https:// 返回 false 并在 039 注记记录；本层为 W2 的 HTTP/3 字节面交付。
//
// 语言能力缺口（不改语言）：`List<T[]>` 元素 Add 损坏 → 数据面以 blob+lengths
// 平坦表示；`long` 字面量不支持 `L` 后缀 → 常数经 `(long)` 显式 cast。
namespace Arc.Net.WebTransport;

using Arc.Collections;
using Arc.Net;

/// <summary>W1/W2 字节面编解码（public 供 e2e harness 驱动；非用户面 API）。</summary>
public static class WebTransportCodec {
    /// <summary>W1：HTTP/2 extended CONNECT 头块（:method=CONNECT :protocol=webtransport）。</summary>
    public static byte[] BuildH2ConnectHeaderBlock(string authority, string path) {
        Http2HeaderList hs = new Http2HeaderList();
        hs.Add(":method", "CONNECT");
        hs.Add(":scheme", "https");
        hs.Add(":authority", authority);
        hs.Add(":path", path);
        hs.Add(":protocol", "webtransport");
        Hpack hpack = new Hpack();
        return hpack.EncodeHeaders(hs);
    }

    /// <summary>W2：HTTP/3 extended CONNECT 头块（:protocol=webtransport-h3）。
    /// 直接经内部 <see cref="Qpack"/> 编码（不依赖已被删除的 Http3Client 公开入口）。</summary>
    public static byte[] BuildH3ConnectHeaderBlock(string authority, string path) {
        Http3HeaderList h = new Http3HeaderList();
        h.Add(":method", "CONNECT");
        h.Add(":scheme", "https");
        h.Add(":authority", authority);
        h.Add(":path", path);
        h.Add(":protocol", "webtransport-h3");
        Qpack qpack = new Qpack();
        return qpack.EncodeHeaders(h);
    }

    /// <summary>W2：客户端控制流整段（流类型 0 CONTROL + SETTINGS：
    /// QPACK 零容量 + ENABLE_CONNECT_PROTOCOL + SETTINGS_H3_DATAGRAM + 草案版
    /// SETTINGS_WT_ENABLED）。SETTINGS 帧自建（S4 Http3Frame.MakeSettings 内部
    /// 经 QuicVarInt.Encode，4 字节 varint 字节序有缺陷——见 039 注记；本层以
    /// WebTransportVarInt 自包含编码）。</summary>
    public static byte[] BuildH3ControlStream() {
        List<long> ids = new List<long>();
        ids.Add((long)0x01);
        ids.Add((long)0x06);
        ids.Add((long)0x07);
        ids.Add((long)0x08);
        ids.Add((long)0x33);
        ids.Add((long)0x2C7CF000);
        List<long> values = new List<long>();
        values.Add((long)0);
        values.Add((long)0);
        values.Add((long)0);
        values.Add((long)1);
        values.Add((long)1);
        values.Add((long)1);
        List<byte> payload = new List<byte>();
        int i = 0;
        while (i < ids.Count) {
            WebTransportVarInt.Encode(payload, ids[i]);
            WebTransportVarInt.Encode(payload, values[i]);
            i = i + 1;
        }
        byte[] pbytes = payload.ToArray();
        List<byte> frame = new List<byte>();
        WebTransportVarInt.Encode(frame, (long)0x04); // SETTINGS 帧类型
        WebTransportVarInt.Encode(frame, (long)pbytes.Length);
        int k = 0;
        while (k < pbytes.Length) {
            frame.Add(pbytes[k]);
            k = k + 1;
        }
        List<byte> out_ = new List<byte>();
        WebTransportVarInt.Encode(out_, (long)0); // CONTROL 流类型
        int j = 0;
        while (j < frame.Count) {
            out_.Add(frame[j]);
            j = j + 1;
        }
        return out_.ToArray();
    }

    /// <summary>W2：HTTP/3 响应头块 → :status（QPACK 解码；未命中/失败返回 0）。</summary>
    public static int ParseH3Status(byte[] headerBlock) {
        Http3HeaderList hs = new Http3HeaderList();
        Qpack qpack = new Qpack();
        if (!qpack.DecodeHeaders(headerBlock, hs)) { return 0; }
        string status = hs.Get(":status");
        if (status == "") { return 0; }
        return Convert.ToInt32(status);
    }

    /// <summary>W2：单向 WebTransport 流字节面（0x54 流类型 + Session ID + 用户数据）。</summary>
    public static byte[] BuildW2UniStream(long sessionId, byte[] payload) {
        List<byte> out_ = new List<byte>();
        WebTransportVarInt.Encode(out_, WebTransportStreamTypes.W2Uni);
        WebTransportVarInt.Encode(out_, sessionId);
        int i = 0;
        while (i < payload.Length) {
            out_.Add(payload[i]);
            i = i + 1;
        }
        return out_.ToArray();
    }

    /// <summary>W2：双向 WebTransport 流首字节信号（0x41 + Session ID + 用户数据）。</summary>
    public static byte[] BuildW2BidiSignal(long sessionId, byte[] payload) {
        List<byte> out_ = new List<byte>();
        WebTransportVarInt.Encode(out_, WebTransportStreamTypes.W2BidiSignal);
        WebTransportVarInt.Encode(out_, sessionId);
        int i = 0;
        while (i < payload.Length) {
            out_.Add(payload[i]);
            i = i + 1;
        }
        return out_.ToArray();
    }

    /// <summary>W2：数据报映射（RFC 9221 · draft-ietf-webtrans-http3-16 §3.4）：
    /// 负载首 varint = Quarter Stream ID（= Session ID / 4），其后为用户数据报载荷。</summary>
    public static byte[] MapDatagram(long sessionId, byte[] payload) {
        List<byte> out_ = new List<byte>();
        WebTransportVarInt.Encode(out_, sessionId / 4);
        int i = 0;
        while (i < payload.Length) {
            out_.Add(payload[i]);
            i = i + 1;
        }
        return out_.ToArray();
    }

    /// <summary>W2：数据报解映射。载荷拷贝到 outPayload（最多 maxLen 字节）并返回实际
    /// 字节数；outQuarter 为 Quarter Stream ID。失败返回 -1。</summary>
    public static int UnmapDatagram(byte[] wire, out long outQuarter, byte[] outPayload, int maxLen) {
        outQuarter = -1;
        if (wire == null) { return -1; }
        int qLen;
        long q = WebTransportVarInt.Decode(wire, 0, out qLen);
        if (q < 0) { return -1; }
        outQuarter = q;
        int avail = wire.Length - qLen;
        int take = avail;
        if (take > maxLen) { take = maxLen; }
        int i = 0;
        while (i < take) {
            outPayload[i] = wire[qLen + i];
            i = i + 1;
        }
        return take;
    }

    /// <summary>W2：WT_CLOSE_SESSION capsule（draft-ietf-webtrans-http3-16 §3.2）。</summary>
    public static byte[] MakeCloseSession(int code, string message) {
        return WebTransportCapsule.MakeCloseSession(code, message);
    }

    /// <summary>W1/W2：DATAGRAM capsule（RFC 9297 §3）。</summary>
    public static byte[] MakeDatagramCapsule(byte[] data) {
        return WebTransportCapsule.MakeDatagram(data);
    }
}
