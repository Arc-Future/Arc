// AsyncStream<T> — 推拉适配器（RFC 008 AsyncStream P1）
namespace Arc.Collections;

using Arc;
using Arc.Threading;

/// <summary>
/// 异步流——把推模型 sink 契约（OnNext/OnCompleted/OnError）适配为拉模型
/// IAsyncEnumerable&lt;T&gt;。内部有界环形缓冲：缓冲满时生产者挂起
///（Monitor.Wait，消费者拉取腾出空间后 Pulse 唤醒），实现天然背压。
///
/// 生命周期契约（对标 System.Threading.Channels 的单消费者模型）：
///   - 生产侧（sink）：网络/IO 线程或回调上下文调用 OnNext 推入；
///     OnCompleted/OnError 终结流（终结后缓冲中已产出值仍可被消费完毕）。
///   - 消费侧：GetAsyncEnumerator 取枚举器后 await foreach / 手写循环拉取。
///   - 枚举器非线程安全且共享同一游标（对齐 C# 单消费者模型）：重复
///     GetAsyncEnumerator 的多个枚举器会互相抢占元素，多消费者禁止共用一个流。
///
/// 注意：缓冲满时生产者阻塞等待——生产与消费不得处于同一线程
/// （单线程 EventLoop 内同侧生产+消费会互等死锁）；AI 流式场景
/// 生产来自 IO 回调线程、消费在主循环，天然满足。
/// </summary>
/// <typeparam name="T">元素类型。</typeparam>
public class AsyncStream<T> : IAsyncEnumerable<T> {
    private Lock _gate;
    private T[] _buffer;
    private int _head;
    private int _tail;
    private int _count;
    private int _capacity;
    private bool _completed;
    private Exception _error;
    private TaskCompletionSource<bool> _waiter;

    /// <summary>以指定缓冲容量创建异步流。</summary>
    /// <param name="capacity">环形缓冲容量（>= 1）；满则生产者挂起。</param>
    public AsyncStream(int capacity) {
        if (capacity < 1) {
            capacity = 1;
        }
        _gate = new Lock();
        _buffer = new T[capacity];
        _capacity = capacity;
    }

    /// <summary>以默认容量（64）创建异步流。</summary>
    public AsyncStream() {
        _gate = new Lock();
        _buffer = new T[64];
        _capacity = 64;
    }

    /// <summary>推入一个元素；缓冲满时挂起直至消费者腾出空间。</summary>
    /// <param name="value">元素值。</param>
    public void OnNext(T value) {
        Monitor.Enter(_gate);
        while (_count == _capacity && !_completed) {
            Monitor.Wait(_gate);
        }
        if (_completed) {
            Monitor.Exit(_gate);
            return;
        }
        _buffer[_tail] = value;
        _tail = (_tail + 1) % _capacity;
        _count++;
        TaskCompletionSource<bool> waiter = _waiter;
        if (waiter != null) {
            _waiter = null;
            waiter.SetResult(true);
        }
        Monitor.Exit(_gate);
    }

    /// <summary>正常终结流；缓冲中已产出值仍可被消费完毕。</summary>
    public void OnCompleted() {
        Monitor.Enter(_gate);
        _completed = true;
        TaskCompletionSource<bool> waiter = _waiter;
        if (waiter != null) {
            _waiter = null;
            waiter.SetResult(false);
        }
        Monitor.PulseAll(_gate);
        Monitor.Exit(_gate);
    }

    /// <summary>以异常终结流；挂起中的 MoveNextAsync 经 FAULTED 通道抛出。</summary>
    /// <param name="error">失败原因。</param>
    public void OnError(Exception error) {
        Monitor.Enter(_gate);
        _error = error;
        _completed = true;
        TaskCompletionSource<bool> waiter = _waiter;
        if (waiter != null) {
            _waiter = null;
            waiter.SetException(error);
        }
        Monitor.PulseAll(_gate);
        Monitor.Exit(_gate);
    }

    /// <summary>获取本流的异步枚举器。</summary>
    /// <param name="cancellationToken">取消令牌（生产者流程取消信号）。</param>
    /// <returns>异步枚举器实例。</returns>
    public IAsyncEnumerator<T> GetAsyncEnumerator(CancellationToken cancellationToken) {
        return new AsyncStreamEnumerator<T>(this);
    }

    /// <summary>枚举器推进核心：缓冲有值同步直通；空且未完成经 TCS 挂起。</summary>
    /// <param name="currentSlot">枚举器的当前值槽（同步路径直接写入）。</param>
    /// <returns>前进成功 true / 序列结束 false 的 Task。</returns>
    internal Task<bool> MoveNextCore(AsyncStreamEnumerator<T> enumerator) {
        Monitor.Enter(_gate);
        if (_count > 0) {
            enumerator.SetCurrent(_buffer[_head]);
            _head = (_head + 1) % _capacity;
            _count--;
            Monitor.Pulse(_gate);
            Monitor.Exit(_gate);
            return Task.FromResult(true);
        }
        if (_error != null) {
            Exception error = _error;
            Monitor.Exit(_gate);
            TaskCompletionSource<bool> tcs = new TaskCompletionSource<bool>();
            tcs.SetException(error);
            return tcs.Task;
        }
        if (_completed) {
            Monitor.Exit(_gate);
            return Task.FromResult(false);
        }
        TaskCompletionSource<bool> waiter = new TaskCompletionSource<bool>();
        _waiter = waiter;
        Monitor.Exit(_gate);
        return waiter.Task;
    }

    /// <summary>同步拉取当前元素（挂起唤醒路径的惰性取值：await 返回 true 时缓冲必有值）。</summary>
    /// <returns>当前元素。</returns>
    internal T TakeCurrent() {
        Monitor.Enter(_gate);
        T value = _buffer[_head];
        _head = (_head + 1) % _capacity;
        _count--;
        Monitor.Pulse(_gate);
        Monitor.Exit(_gate);
        return value;
    }
}

/// <summary>
/// AsyncStream 的异步枚举器——单消费者契约（非线程安全）。
/// Current 惰性拉取：同步推进路径值已缓存；挂起唤醒路径经 TakeCurrent
/// 从缓冲取（await true 返回时缓冲必有值，取值无阻塞）。
/// </summary>
/// <typeparam name="T">元素类型。</typeparam>
public class AsyncStreamEnumerator<T> : IAsyncEnumerator<T> {
    private AsyncStream<T> _stream;
    private T _current;
    private bool _hasCurrent;

    /// <summary>绑定流创建枚举器。</summary>
    /// <param name="stream">被枚举的流。</param>
    public AsyncStreamEnumerator(AsyncStream<T> stream) {
        _stream = stream;
    }

    /// <summary>驱动到下一个元素。</summary>
    /// <returns>前进成功返回 true；越过序列末尾返回 false。</returns>
    public Task<bool> MoveNextAsync() {
        _hasCurrent = false;
        return _stream.MoveNextCore(this);
    }

    /// <summary>写入当前元素缓存（MoveNextCore 同步直通路径回调）。</summary>
    /// <param name="value">当前元素。</param>
    public void SetCurrent(T value) {
        _current = value;
        _hasCurrent = true;
    }

    /// <summary>当前元素；挂起唤醒路径惰性从流缓冲拉取。</summary>
    public T Current {
        get {
            if (!_hasCurrent) {
                _current = _stream.TakeCurrent();
                _hasCurrent = true;
            }
            return _current;
        }
    }
}
