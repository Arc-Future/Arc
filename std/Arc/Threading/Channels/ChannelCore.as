// ChannelCore<T> — 通道状态机核心（RFC 046）。
namespace Arc.Threading.Channels;

using Arc;
using Arc.Collections;
using Arc.Threading;

/// <summary>
/// 通道状态机核心——有界/无界共用实现。全部状态迁移经 Monitor（Lock 对象）
/// 串行化；挂起经 TCS 门（SetResult/SetException 均为已验证通道）；缓冲为
/// 环形数组，无界通道倍增扩容。
///
/// 交接协议（三职责均为锁内原子步骤，杜绝丢失唤醒与竞态重试环）：
///   - 直付：写元素时存在未终结读等待者 → 绕过缓冲直接 SetResult 交付；
///   - 空位回收：读出队后存在未终结写等待者 → 其元素收纳入缓冲并唤醒
///     （Wait 模式 O(1) 背压释放，元素先入缓冲后唤醒，无竞态窗口）；
///   - 完成判定：终结且缓冲排尽 → 完成信号 settle（有 error 则失败）。
///
/// 等待者队列以显式计数器（_readerWaiters/_writerWaiters）控长——
/// Queue&lt;T&gt;.Count 经可达性裁剪后存在未定义符号缺口（arc-prune-001）。
/// </summary>
/// <typeparam name="T">元素类型。</typeparam>
internal class ChannelCore<T> {
    private Lock _gate;
    private T[] _items;
    private int _head;
    private int _count;
    private int _limit;
    private bool _unbounded;
    private BoundedChannelFullMode _mode;
    private Queue<ChannelReaderWaiter<T>> _readers;
    private Queue<ChannelWriterWaiter<T>> _writers;
    private int _readerWaiters;
    private int _writerWaiters;
    private bool _completed;
    private Exception? _error;
    private TaskCompletionSource<bool> _completion;
    private bool _completionSettled;

    /// <summary>以容量与背压模式构造核心；unbounded 时容量参数被忽略（写端永不等待）。</summary>
    /// <param name="capacity">缓冲容量上限（unbounded 时须为 0）。</param>
    /// <param name="mode">背压模式。</param>
    /// <param name="unbounded">是否无界（容量校验与空间判定由此分流）。</param>
    public ChannelCore(int capacity, BoundedChannelFullMode mode, bool unbounded) {
        if (!unbounded && capacity <= 0) {
            throw new ArgumentException("capacity must be greater than zero");
        }
        _gate = new Lock();
        _limit = capacity;
        _unbounded = unbounded;
        if (_unbounded) {
            _items = new T[16];
        } else {
            _items = new T[capacity];
        }
        _mode = mode;
        _readers = new Queue<ChannelReaderWaiter<T>>();
        _writers = new Queue<ChannelWriterWaiter<T>>();
        _completion = new TaskCompletionSource<bool>();
    }

    /// <summary>完成信号 Task（终结且排尽后 settle；读写端共用）。</summary>
    public Task<bool> CompletionGate { get { return _completion.Task; } }

    /// <summary>缓冲中当前元素数（直付中的元素不计入）。</summary>
    public int Count {
        get {
            Monitor.Enter(_gate);
            int count = _count;
            Monitor.Exit(_gate);
            return count;
        }
    }

    // ── 写路径 ──

    /// <summary>同步写：入队或直付读等待者；满按背压模式；终结后 false。</summary>
    /// <param name="item">元素值。</param>
    /// <returns>true 表示通道已接收（drop 模式下可能已按策略丢弃）。</returns>
    public bool TryWrite(T item) {
        Monitor.Enter(_gate);
        if (_completed) {
            Monitor.Exit(_gate);
            return false;
        }
        if (_unbounded || _count < _limit) {
            ChannelReaderWaiter<T>? wake = this.ServeReader(item);
            if (wake == null) {
                this.EnqueueItem(item);
                ChannelDiag.CountTryWriteBuffered();
            } else {
                ChannelDiag.CountTryWriteDirect();
            }
            Monitor.Exit(_gate);
            if (wake != null) {
                wake.Gate.SetResult(item);
            }
            return true;
        }
        if (_mode == BoundedChannelFullMode.DropOldest) {
            this.DequeueItem();
            ChannelReaderWaiter<T>? wake = this.ServeReader(item);
            if (wake == null) {
                this.EnqueueItem(item);
            }
            Monitor.Exit(_gate);
            if (wake != null) {
                wake.Gate.SetResult(item);
            }
            return true;
        }
        bool accepted = _mode != BoundedChannelFullMode.Wait;
        Monitor.Exit(_gate);
        if (!accepted) {
            ChannelDiag.CountTryWriteRejected();
        }
        return accepted;
    }

    /// <summary>登记写等待者（满载 Wait 模式慢路径）；锁内竞态回收（已腾出空位）时即时收纳唤醒；终结抛出。</summary>
    /// <param name="waiter">携带待写元素的等待者。</param>
    /// <returns>挂起门 Task（竞态回收路径为已完成态）。</returns>
    public Task<bool> WriteEnqueue(ChannelWriterWaiter<T> waiter) {
        Monitor.Enter(_gate);
        if (_completed) {
            Monitor.Exit(_gate);
            throw new ChannelClosedException("The write operation was rejected because the channel was completed.");
        }
        if (_count < _limit) {
            this.EnqueueItem(waiter.Item);
            waiter.Done = true;
            waiter.Served = true;
            waiter.Gate.SetResult(true);
            ChannelDiag.CountWriteEnqueueSync();
        } else {
            _writers.Enqueue(waiter);
            _writerWaiters = _writerWaiters + 1;
            ChannelDiag.CountWriteEnqueueRegistered();
        }
        Monitor.Exit(_gate);
        return waiter.Gate.Task;
    }

    /// <summary>取消挂起写等待者（ct 回调；幂等；哨兵唤醒后元素不会被写入）。</summary>
    /// <param name="waiter">目标等待者。</param>
    public void CancelWrite(ChannelWriterWaiter<T> waiter) {
        Monitor.Enter(_gate);
        if (waiter.Done) {
            Monitor.Exit(_gate);
            return;
        }
        waiter.Done = true;
        waiter.Gate.SetResult(false);
        Monitor.Exit(_gate);
    }

    // ── 读路径 ──

    /// <summary>同步读：缓冲有值出队（含空位回收与完成判定）；空返回 false。</summary>
    /// <param name="item">出队元素。</param>
    /// <returns>true 表示取得元素。</returns>
    public bool TryRead(out T item) {
        item = default(T);
        Monitor.Enter(_gate);
        if (_count > 0) {
            item = this.DequeueItem();
            this.AdmitWriters();
            this.SettleCompletion();
            Monitor.Exit(_gate);
            return true;
        }
        Monitor.Exit(_gate);
        return false;
    }

    /// <summary>登记读等待者（空载慢路径）；锁内竞态回收（元素已到）时同步完成；终结排尽抛出。</summary>
    /// <param name="waiter">出参：登记的等待者（同步完成路径为 null）。</param>
    /// <returns>挂起门 Task（同步完成路径为已完成 Task）。</returns>
    public Task<T> ReadEnqueue(out ChannelReaderWaiter<T>? waiter) {
        Monitor.Enter(_gate);
        if (_count > 0) {
            T value = this.DequeueItem();
            this.AdmitWriters();
            this.SettleCompletion();
            Monitor.Exit(_gate);
            waiter = null;
            ChannelDiag.CountReadEnqueueSync();
            return Task.FromResult(value);
        }
        if (_completed) {
            Monitor.Exit(_gate);
            waiter = null;
            if (_error != null) {
                throw _error;
            }
            throw new ChannelClosedException("The channel has been completed and drained; no more items will be read.");
        }
        ChannelReaderWaiter<T> pending = new ChannelReaderWaiter<T>();
        _readers.Enqueue(pending);
        _readerWaiters = _readerWaiters + 1;
        ChannelDiag.CountReadEnqueueRegistered();
        Monitor.Exit(_gate);
        waiter = pending;
        return pending.Gate.Task;
    }

    /// <summary>取消挂起读等待者（ct 回调；幂等；以 default(T) 哨兵唤醒，rt_arc_inc 对 null 安全）。</summary>
    /// <param name="waiter">目标等待者。</param>
    public void CancelRead(ChannelReaderWaiter<T>? waiter) {
        if (waiter == null) {
            return;
        }
        Monitor.Enter(_gate);
        if (waiter.Done) {
            Monitor.Exit(_gate);
            return;
        }
        waiter.Done = true;
        waiter.Gate.SetResult(default(T));
        Monitor.Exit(_gate);
    }

    // ── 终结 ──

    /// <summary>终结通道：挂起读端以复查唤醒（重查缓冲余量——终结前已写入数据仍可
    /// 消费，排尽后经 ReadEnqueue 抛出终结异常）、挂起写端以
    /// ChannelClosedException 失败，并判定完成信号；重复终结抛出。</summary>
    /// <param name="error">终结原因（null 表示正常终结）。</param>
    public void Complete(Exception? error) {
        Monitor.Enter(_gate);
        if (_completed) {
            Monitor.Exit(_gate);
            throw new ChannelClosedException("The channel has already been completed.");
        }
        _completed = true;
        _error = error;
        while (_readerWaiters > 0) {
            ChannelReaderWaiter<T> reader = _readers.Dequeue();
            _readerWaiters = _readerWaiters - 1;
            if (reader.Done) {
                continue;
            }
            reader.Done = true;
            reader.Recheck = true;
            reader.Gate.SetResult(default(T));
            ChannelDiag.CountCompleteRecheck();
        }
        ChannelClosedException writeError = new ChannelClosedException("The write operation was canceled because the channel was completed.");
        while (_writerWaiters > 0) {
            ChannelWriterWaiter<T> writer = _writers.Dequeue();
            _writerWaiters = _writerWaiters - 1;
            if (writer.Done) {
                continue;
            }
            writer.Done = true;
            writer.Gate.SetException(writeError);
            ChannelDiag.CountCompleteWriterFail();
        }
        this.SettleCompletion();
        Monitor.Exit(_gate);
    }

    // ── 锁内职责（仅在持有 _gate 时调用）──

    private T DequeueItem() {
        ChannelDiag.CountDequeue();
        T value = _items[_head];
        _items[_head] = default(T);
        _head = (_head + 1) % _items.Length;
        _count = _count - 1;
        return value;
    }

    private void EnqueueItem(T item) {
        if (_count == _items.Length) {
            this.Grow();
        }
        _items[(_head + _count) % _items.Length] = item;
        _count = _count + 1;
    }

    private void Grow() {
        T[] grown = new T[_items.Length * 2];
        for (int i = 0; i < _count; i++) {
            grown[i] = _items[(_head + i) % _items.Length];
        }
        _items = grown;
        _head = 0;
    }

    /// <summary>直付预约：摘出首个未终结读等待者并标记，返回交调用方退锁后
    /// SetResult 唤醒（持锁唤醒与 Monitor 的交互为运行期崩溃嫌疑点）；
    /// 无等待者返回 null。</summary>
    private ChannelReaderWaiter<T>? ServeReader(T item) {
        while (_readerWaiters > 0) {
            ChannelReaderWaiter<T> waiter = _readers.Peek();
            if (waiter.Done) {
                _readers.Dequeue();
                _readerWaiters = _readerWaiters - 1;
                continue;
            }
            _readers.Dequeue();
            _readerWaiters = _readerWaiters - 1;
            waiter.Done = true;
            waiter.Served = true;
            ChannelDiag.CountServeReader();
            return waiter;
        }
        return null;
    }

    /// <summary>空位回收：把首个未终结写等待者的元素收纳进缓冲并唤醒（终结后不再收纳）。</summary>
    private void AdmitWriters() {
        while (_writerWaiters > 0 && !_completed && _count < _limit) {
            ChannelWriterWaiter<T> waiter = _writers.Peek();
            if (waiter.Done) {
                _writers.Dequeue();
                _writerWaiters = _writerWaiters - 1;
                continue;
            }
            _writers.Dequeue();
            _writerWaiters = _writerWaiters - 1;
            this.EnqueueItem(waiter.Item);
            waiter.Done = true;
            waiter.Served = true;
            waiter.Gate.SetResult(true);
            ChannelDiag.CountAdmitWriters();
        }
    }

    /// <summary>完成判定：终结且缓冲排尽后 settle 完成信号（有 error 则失败）。</summary>
    private void SettleCompletion() {
        if (_completed && _count == 0 && !_completionSettled) {
            _completionSettled = true;
            ChannelDiag.CountSettle();
            if (_error != null) {
                _completion.SetException(_error);
            } else {
                _completion.SetResult(true);
            }
        }
    }
}
