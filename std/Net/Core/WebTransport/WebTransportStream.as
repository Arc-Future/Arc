// RFC 039 W1/W2: Arc.Net.WebTransport — WebTransport 流（byte[] 数据面）。
//
// 流语义（039 §1.3 签名级定稿 · draft-ietf-webtrans-http2-15 §5.2）：
//   - W1：流经 WT_STREAM / WT_STREAM_FIN capsule 复用在 HTTP/2 extended CONNECT
//     流上；流 ID 镜像 QUIC：客户端发起 = 偶数（双向 0,4,8… / 单向 2,6,10…），
//     服务器发起 = 奇数；第二最低位指示方向（0 = 双向，2 = 单向）。
//   - W2：原生 QUIC 双向/单向流（draft-ietf-webtrans-http3-16 §3.1），传输由
//     e2e harness 直调 rt_quic_* 承载；本类型为 Arc 侧统一表达。
//   - Read 返回实际读出字节数（0 = 当前无可读字节且未 FIN）；Write 立即组
//     WT_STREAM capsule 落线（同步写，对齐 NetworkStream 全量写语义）；CloseAsync
//     发送 WT_STREAM_FIN 并等对端回应后完成单流关闭。
//
// 载体纪律（039 §1.2）：全程 byte[]。语言能力缺口（不改语言）：`byte[]` 字段直读
// 不支持 .Length/索引 → 先拷贝局部。
namespace Arc.Net.WebTransport;

using Arc.Collections;

public class WebTransportStream : IDisposable {
    private int _streamId;
    private List<byte> _readQueue;
    private int _readCursor;
    private bool _readFinished;
    private bool _closed;
    private WebTransportClient _owner;
    private bool _peerInitiated;
    private bool _accepted;

    internal WebTransportStream(WebTransportClient owner, int streamId) {
        _owner = owner;
        _streamId = streamId;
        _readQueue = new List<byte>();
        _readCursor = 0;
        _readFinished = false;
        _closed = false;
        _peerInitiated = false;
        _accepted = false;
    }

    /// <summary>流标识：W1 下为 WebTransport 流号（客户端发起偶数 / 服务器发起奇数）；
    /// W2 下为 QUIC 流号。</summary>
    public int StreamId {
        get { return _streamId; }
    }

    /// <summary>对端发起（服务器→客户端方向；Accept 面返回的流恒为 true）。</summary>
    internal bool IsPeerInitiated {
        get { return _peerInitiated; }
        set { _peerInitiated = value; }
    }

    /// <summary>已被 Accept 面取走。</summary>
    internal bool Accepted {
        get { return _accepted; }
        set { _accepted = value; }
    }

    /// <summary>数据已耗尽（对端 FIN）且缓存读空。</summary>
    public bool IsReadComplete {
        get { return _readFinished && _readCursor >= _readQueue.Count; }
    }

    /// <summary>把对端流入字节追加到读队列（W1 引擎增量交付；W2 字节面填充）。</summary>
    internal void Deliver(byte[] data) {
        if (data == null) { return; }
        int i = 0;
        while (i < data.Length) {
            _readQueue.Add(data[i]);
            i = i + 1;
        }
    }

    /// <summary>对端流入单字节追加（W1 引擎逐字节增量交付）。</summary>
    internal void DeliverChunk(byte b) {
        _readQueue.Add(b);
    }

    /// <summary>对端 FIN：标记读完成。</summary>
    internal void MarkFin() {
        _readFinished = true;
    }

    /// <summary>读取至多 count 字节到 buffer[offset..]；返回实际字节数（0 = 无缓存且
    /// 未 FIN）。纯同步语义（连接读循环异步推进，本地闭环下缓存已就绪）。</summary>
    public int Read(byte[] buffer, int offset, int count) {
        if (buffer == null || count <= 0) { return 0; }
        int available = _readQueue.Count - _readCursor;
        if (available <= 0) { return 0; }
        int take = available;
        if (take > count) { take = count; }
        int i = 0;
        while (i < take) {
            buffer[offset + i] = _readQueue[_readCursor + i];
            i = i + 1;
        }
        _readCursor = _readCursor + take;
        if (_readCursor >= _readQueue.Count) {
            _readQueue.Clear();
            _readCursor = 0;
        }
        return take;
    }

    /// <summary>全量写（byte[] · N3）：立即组 WT_STREAM capsule 发送（W1）。
    /// 连接未建立/失败时静默丢弃（同步面无错误传播）。</summary>
    public void Write(byte[] buffer, int offset, int count) {
        if (_closed || _owner == null || buffer == null || count <= 0) { return; }
        _owner.StreamWrite(_streamId, buffer, offset, count);
    }

    /// <summary>单流关闭（FIN）：发送 WT_STREAM_FIN 并等对端回应（本地闭环握手）。</summary>
    public Task CloseAsync() {
        if (_closed) { return Task.CompletedTask; }
        _closed = true;
        if (_owner == null) { return Task.CompletedTask; }
        _owner.StreamCloseRequest(_streamId);
        return Task.CompletedTask;
    }

    /// <summary>内部：会话关闭时置位（防悬挂）。</summary>
    internal void MarkClosed() {
        _closed = true;
        _readFinished = true;
    }

    public void Dispose() {
        _closed = true;
    }
}
