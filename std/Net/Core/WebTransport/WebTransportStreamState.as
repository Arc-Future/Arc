// WebTransportStreamState —— 拆分自 WebTransportTypes.as（一文件一公开类型）。
namespace Arc.Net.WebTransport;
using Arc.Collections;

/// <summary>单向 WebTransport 流（W1：WT_STREAM capsule 携带的流；W2：0x54 流）。</summary>
internal class WebTransportStreamState {
    public int StreamId;
    public List<byte> Inbound;
    public bool ReadFinished;
    public bool Closed;

    public WebTransportStreamState(int streamId) {
        StreamId = streamId;
        Inbound = new List<byte>();
        ReadFinished = false;
        Closed = false;
    }
}
