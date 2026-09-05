// RFC 042 M1 (M1b): Yamux — yamux/1.0.0 会话（真实流复用）。
//
// 真实面（对齐 yamux/1.0.0 wire / go-yamux 语义）：
//   - 帧：12 字节头（ver/type/flags/streamID/length，大端）+ 可选载荷。
//   - 流：单 TCP 连接承载多逻辑流；streamID 奇数=拨号方开启、偶数=监听方开启；
//     0 为控制流（Ping/GoAway）。位运算（&/|/<<）解析与构建帧头。
//   - 流控：WindowUpdate 帧携带授予对端的发送额度；发送受窗口约束、耗尽等待补发；
//     接收消费后达阈值补发 WindowUpdate（defaultWindow=256KiB，对齐 go-yamux 默认）。
//   - 出帧互斥：全部出帧（含 reader 线程 ACK/RST）经会话级 Semaphore(1) 串行化；
//     Data 帧头与载荷拼单缓冲一次发送，保证帧原子性。
//   - 后台 reader 线程解复用帧 → 各流 BlockingCollection 接收队列（线程安全）。
//   - 底层 TcpClient.SendBytes/ReceiveBytes（显式长度、无 NUL 截断，二进制安全）。
//
// 诚实边界（M1b 切片；对齐 RFC 042 M1 触碰面）：

namespace Arc.Net.P2P;
using Arc;
using Arc.Net;
using Arc.Collections;
using Arc.Collections.Concurrent;
using Arc.Threading;
using Arc.Text;

/// <summary>yamux/1.0.0 会话：帧收发 + 流表 + 窗口 + 后台 reader 解复用。</summary>
public class YamuxSession {
    private TcpClient _tcp;
    private bool _isServer;
    private Lock _lock;
    private Dictionary<int, YamuxStream> _streams;
    private BlockingCollection<YamuxStream> _acceptQueue;
    private int _nextStreamId;
    private Thread _reader;
    private bool _running;
    private Semaphore _sendMutex;

    /// <param name="client">已建立连接的 TcpClient（二进制安全 byte[] 面）。</param>
    /// <param name="isServer">true=监听方（偶数 streamID），false=拨号方（奇数）。</param>
    public YamuxSession(TcpClient client, bool isServer) {
        _tcp = client;
        _isServer = isServer;
        _lock = new Lock();
        _streams = new Dictionary<int, YamuxStream>();
        _acceptQueue = new BlockingCollection<YamuxStream>(0);
        _nextStreamId = isServer ? 2 : 1;
        _reader = null;
        _running = true;
        _sendMutex = new Semaphore(1, 1);
        IsClosed = false;
    }

    /// <summary>会话是否已关闭。</summary>
    public bool IsClosed { get; set; }

    /// <summary>启动后台 reader（首个流操作前调用一次）。</summary>
    private void EnsureReader() {
        lock (_lock) {
            if (_reader == null && !IsClosed) {
                _running = true;
                _reader = new Thread(() => this.ReadLoop());
                _reader.Start();
            }
        }
    }

    // ── 流开启 / 接受 ──

    /// <summary>开启一条本地流（按角色分配奇数/偶数 streamID）。</summary>
    public YamuxStream OpenStream() {
        this.EnsureReader();
        int sid;
        YamuxStream s = null;
        lock (_lock) {
            sid = _nextStreamId;
            _nextStreamId = _nextStreamId + 2;
            s = new YamuxStream(this, sid, 0);
            _streams.Add(sid, s);
        }
        // WindowUpdate+SYN：通告我方接收窗口并请求对端开流（length=授予对端发送额度）。
        this.SendHeader(YamuxConst.TypeWindowUpdate, YamuxConst.FlagSyn, sid, YamuxConst.DefaultWindow);
        return s;
    }

    /// <summary>接受一条入站流（阻塞直到有流或会话关闭返回 null）。</summary>
    public YamuxStream AcceptStream() {
        this.EnsureReader();
        return _acceptQueue.Take();
    }

    // ── 流 I/O（由 YamuxStream 调用）──

    internal bool WriteStream(YamuxStream s, string data) {
        byte[] body = Encoding.GetBytes(data);
        int len = body.Length;
        int sent = 0;
        while (sent < len) {
            int want = len - sent;
            if (want > YamuxConst.MaxFrame) {
                want = YamuxConst.MaxFrame;
            }
            int chunk = this.AwaitSendWindow(s, want);
            if (chunk <= 0) {
                return false;
            }
            if (!this.SendData(s.StreamId, body, sent, chunk)) {
                return false;
            }
            sent = sent + chunk;
        }
        return true;
    }

    /// <summary>
    /// 等待并原子扣减发送额度，返回实际可发字节数（0=会话关闭或约 4s 无额度）。
    /// 唤醒面为流级 Semaphore：WindowUpdate 到达 Release 一次；扣减后仍有余量
    /// 则重臂一次（多等待者唤醒链）。等待时不持 _lock 与发送互斥（死锁纪律）。
    /// </summary>
    private int AwaitSendWindow(YamuxStream s, int want) {
        int waited = 0;
        while (true) {
            if (IsClosed) {
                return 0;
            }
            int take = 0;
            bool rearm = false;
            lock (_lock) {
                int w = s.GetSendWindow();
                if (w > 0) {
                    take = w;
                    if (want < w) {
                        take = want;
                    }
                    s.AddSendWindow(-take);
                    rearm = s.GetSendWindow() > 0;
                }
            }
            if (take > 0) {
                if (rearm) {
                    s.ReleaseWindowSignal();
                }
                return take;
            }
            if (waited >= 4000) {
                return 0;
            }
            s.WaitWindowSignal(100);
            waited = waited + 100;
        }
    }

    internal bool CloseWriteStream(YamuxStream s) {
        return this.SendHeader(YamuxConst.TypeData, YamuxConst.FlagFin, s.StreamId, 0);
    }

    internal void CloseStream(YamuxStream s) {
        this.SendHeader(YamuxConst.TypeWindowUpdate, YamuxConst.FlagRst, s.StreamId, 0);
    }

    /// <summary>接收侧消费后补发窗口（累积达阈值一次性补发）。</summary>
    internal void OnConsumed(YamuxStream s, int bytes) {
        int reclaim = 0;
        lock (_lock) {
            s.AddRecvReclaimed(bytes);
            if (s.GetRecvReclaimed() >= YamuxConst.DefaultWindow / 2) {
                reclaim = s.GetRecvReclaimed();
                s.ResetRecvReclaimed();
            }
        }
        if (reclaim > 0) {
            this.SendHeader(YamuxConst.TypeWindowUpdate, 0, s.StreamId, reclaim);
        }
    }

    // ── 后台 reader：解复用帧 → 各流队列 ──

    private void ReadLoop() {
        byte[] header = new byte[YamuxConst.HeaderSize];
        while (_running) {
            int hn = this.ReadExact(header, YamuxConst.HeaderSize);
            if (hn < YamuxConst.HeaderSize) {
                break;
            }
            int type = header[1];
            int flags = ((header[2] & 0xFF) << 8) | (header[3] & 0xFF);
            int streamId = ((header[4] & 0xFF) << 24) | ((header[5] & 0xFF) << 16)
                          | ((header[6] & 0xFF) << 8) | (header[7] & 0xFF);
            int length = ((header[8] & 0xFF) << 24) | ((header[9] & 0xFF) << 16)
                        | ((header[10] & 0xFF) << 8) | (header[11] & 0xFF);

            if (type == YamuxConst.TypePing) {
                // Ping/Ping-ACK 无载荷：length 字段为 opaque 32-bit 值，ACK 原样回填。
                bool ack = (flags & YamuxConst.FlagAck) != 0;
                if (!ack) {
                    this.SendHeader(YamuxConst.TypePing, YamuxConst.FlagAck, 0, length);
                }
                continue;
            }

            if (type == YamuxConst.TypeGoAway) {
                break;
            }

            if (type == YamuxConst.TypeWindowUpdate) {
                if (streamId == 0) {
                    continue;
                }
                bool syn = (flags & YamuxConst.FlagSyn) != 0;
                YamuxStream s = this.GetStream(streamId);
                if (s == null && syn) {
                    // 入站新流：length=对端授予我方发送额度；回 ACK 授予对端额度。
                    s = new YamuxStream(this, streamId, length);
                    lock (_lock) {
                        _streams.Add(streamId, s);
                    }
                    _acceptQueue.Add(s);
                    this.SendHeader(YamuxConst.TypeWindowUpdate, YamuxConst.FlagAck, streamId, YamuxConst.DefaultWindow);
                } else if (s != null && length > 0) {
                    lock (_lock) {
                        s.AddSendWindow(length);
                    }
                    s.ReleaseWindowSignal();
                }
                continue;
            }

            if (type == YamuxConst.TypeData) {
                if (length > YamuxConst.MaxReadFrame) {
                    break;
                }
                byte[] payload = this.ReadPayload(length);
                if (payload == null) {
                    break;
                }
                bool fin = (flags & YamuxConst.FlagFin) != 0;
                YamuxStream s = this.GetStream(streamId);
                if (s == null) {
                    this.SendHeader(YamuxConst.TypeWindowUpdate, YamuxConst.FlagRst, streamId, 0);
                    continue;
                }
                if (length > 0) {
                    s.PushData(payload);
                }
                if (fin) {
                    s.PushData(null);   // EOF 哨兵
                }
                continue;
            }

            // 未知类型：读掉载荷保持帧同步（length 视为载荷长）。
            if (length > YamuxConst.MaxReadFrame) {
                break;
            }
            byte[] rest = this.ReadPayload(length);
            if (rest == null) {
                break;
            }
        }

        // reader 退出：会话视为关闭，唤醒接受队列。
        lock (_lock) {
            _running = false;
            IsClosed = true;
        }
        _acceptQueue.Add(null);
    }

    private byte[] ReadPayload(int length) {
        if (length == 0) {
            return new byte[0];
        }
        byte[] payload = new byte[length];
        int gn = this.ReadExact(payload, length);
        if (gn < length) {
            return null;
        }
        return payload;
    }

    private YamuxStream GetStream(int streamId) {
        lock (_lock) {
            if (_streams.ContainsKey(streamId)) {
                return _streams[streamId];
            }
            return null;
        }
    }

    private int ReadExact(byte[] buf, int count) {
        int got = 0;
        while (got < count) {
            int n = _tcp.ReceiveBytes(buf, got, count - got);
            if (n <= 0) {
                return got;
            }
            got = got + n;
        }
        return got;
    }

    private bool SendRaw(byte[] data) {
        if (!this.EnterSend()) {
            return false;
        }
        try {
            int sent = 0;
            while (sent < data.Length) {
                int n = _tcp.SendBytes(data, sent, data.Length - sent);
                if (n <= 0) {
                    return false;
                }
                sent = sent + n;
            }
            return true;
        } finally {
            _sendMutex.Release();
        }
    }

    /// <summary>进入发送临界区（Semaphore(1)；会话关闭后 100ms 内退出等待）。</summary>
    private bool EnterSend() {
        while (true) {
            if (IsClosed) {
                return false;
            }
            if (_sendMutex.Wait(100)) {
                return true;
            }
        }
    }

    private bool SendHeader(int type, int flags, int streamId, int length) {
        byte[] header = YamuxCodec.EncodeHeader(type, flags, streamId, length);
        return this.SendRaw(header);
    }

    private bool SendData(int streamId, byte[] payload, int offset, int count) {
        byte[] header = YamuxCodec.EncodeHeader(YamuxConst.TypeData, 0, streamId, count);
        byte[] frame = new byte[YamuxConst.HeaderSize + count];
        Array.Copy(header, 0, frame, 0, YamuxConst.HeaderSize);
        if (count > 0) {
            Array.Copy(payload, offset, frame, YamuxConst.HeaderSize, count);
        }
        return this.SendRaw(frame);
    }

    // ── 关闭 ──

    /// <summary>关闭会话：关 TCP 使 reader 退出，唤醒所有等待。</summary>
    public void Close() {
        lock (_lock) {
            if (IsClosed) {
                return;
            }
            IsClosed = true;
            _running = false;
        }
        _tcp.Close();
        _acceptQueue.Add(null);
        int[] ids = null;
        lock (_lock) {
            ids = _streams.Keys;
        }
        int i = 0;
        while (i < ids.Length) {
            YamuxStream s = this.GetStream(ids[i]);
            if (s != null) {
                s.PushData(null);
            }
            i = i + 1;
        }
    }
}

