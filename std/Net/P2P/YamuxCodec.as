// YamuxCodec —— 拆分自 Yamux.as（一文件一公开类型）。
namespace Arc.Net.P2P;
using Arc;
using Arc.Net;
using Arc.Collections;
using Arc.Collections.Concurrent;
using Arc.Threading;
using Arc.Text;

/// <summary>yamux 帧头编解码（12 字节大端；位运算组装/提取）。</summary>
internal class YamuxCodec {
    /// <summary>编码 12 字节帧头。</summary>
    public static byte[] EncodeHeader(int type, int flags, int streamId, int length) {
        byte[] h = new byte[YamuxConst.HeaderSize];
        h[0] = (byte)0;                          // version（0）
        h[1] = (byte)type;
        h[2] = (byte)((flags >> 8) & 0xFF);
        h[3] = (byte)(flags & 0xFF);
        h[4] = (byte)((streamId >> 24) & 0xFF);
        h[5] = (byte)((streamId >> 16) & 0xFF);
        h[6] = (byte)((streamId >> 8) & 0xFF);
        h[7] = (byte)(streamId & 0xFF);
        h[8] = (byte)((length >> 24) & 0xFF);
        h[9] = (byte)((length >> 16) & 0xFF);
        h[10] = (byte)((length >> 8) & 0xFF);
        h[11] = (byte)(length & 0xFF);
        return h;
    }
}
