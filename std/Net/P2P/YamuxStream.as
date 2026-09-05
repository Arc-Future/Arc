// YamuxStream —— 拆分自 Yamux.as（一文件一公开类型）。
namespace Arc.Net.P2P;
using Arc;
using Arc.Net;
using Arc.Collections;
using Arc.Collections.Concurrent;
using Arc.Threading;
using Arc.Text;

/// <summary>
/// yamux 逻辑流——IStream 实现（string 公共面；内部二进制帧）。
/// 同步读写内核 + 异步接口包裹（对齐 Http2Connection 的 Task 包裹惯例）。
/// </summary>
public class YamuxStream : IStream {
    private YamuxSession _session;
    private BlockingCollection<byte[]> _recvQueue;
    private bool _eof;

    // 窗口状态（由 _session 锁保护读写）；_windowSignal 为窗口唤醒面（0 起始计数信号量）。
    private int _sendWindow;
    private int _recvReclaimed;
    private Semaphore _windowSignal;

    public YamuxStream(YamuxSession session, int streamId, int sendWindow) {
        _session = session;
        StreamId = streamId;
        _recvQueue = new BlockingCollection<byte[]>(0);
        _eof = false;
        _sendWindow = sendWindow;
        _recvReclaimed = 0;
        _windowSignal = new Semaphore(0, 2147483647);
    }

    /// <summary>流标识（yamux streamID）。</summary>
    public int StreamId { get; }

    // ── 内部（供 reader / session 访问；窗口读写须持锁）──

    internal void PushData(byte[] data) {
        _recvQueue.Add(data);
    }

    internal int GetSendWindow() {
        return _sendWindow;
    }

    internal void AddSendWindow(int delta) {
        _sendWindow = _sendWindow + delta;
    }

    internal void WaitWindowSignal(int milliseconds) {
        _windowSignal.Wait(milliseconds);
    }

    internal void ReleaseWindowSignal() {
        _windowSignal.Release();
    }

    internal int GetRecvReclaimed() {
        return _recvReclaimed;
    }

    internal void AddRecvReclaimed(int n) {
        _recvReclaimed = _recvReclaimed + n;
    }

    internal void ResetRecvReclaimed() {
        _recvReclaimed = 0;
    }

    // ── 同步读写面（e2e 与内部使用；二进制安全）──

    /// <summary>写入字符串载荷（分片 + 窗口约束，循环补齐）。失败返回 false。</summary>
    public bool Write(string data) {
        return _session.WriteStream(this, data);
    }

    /// <summary>阻塞读取一段数据；EOF（对端关闭/会话关闭）返回 null。</summary>
    public string Read() {
        if (_eof) {
            return null;
        }
        byte[] chunk = _recvQueue.Take();
        if (chunk == null) {
            _eof = true;
            return null;
        }
        _session.OnConsumed(this, chunk.Length);
        return Encoding.GetString(chunk);
    }

    /// <summary>半关闭写侧：发送 FIN。</summary>
    public bool CloseWrite() {
        return _session.CloseWriteStream(this);
    }

    /// <summary>关闭流（发送 RST 并标记本地关闭）。</summary>
    public void Close() {
        _session.CloseStream(this);
    }

    // ── IStream 接口实现（异步包裹同步内核）──

    public async Task<void> WriteAsync(string data, CancellationToken cancellationToken) {
        this.Write(data);
    }

    public async Task<string> ReadAsync(CancellationToken cancellationToken) {
        return this.Read();
    }

    public async Task<void> CloseWriteAsync(CancellationToken cancellationToken) {
        this.CloseWrite();
    }

    public async Task<void> CloseAsync(CancellationToken cancellationToken) {
        this.Close();
    }
}
