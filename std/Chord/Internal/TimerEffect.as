// TimerEffect —— 定时回调（RFC 045 D10）。
//
// 专用工作线程 + 协作式取消：线程分段睡眠轮询取消标志（10ms 粒度），
// 取消经 Dispose 置标志后 Join 回收；回调执行期间不中断（协作式）。
// 回调在工作线程执行——内核状态机操作须由使用者回送主线程。
namespace Arc.Chord;

using Arc;
using Arc.Collections;
using Arc.Threading;


internal class TimerEffect : IDisposable {
    private Action _callback;
    private int _delayMs;
    private bool _repeat;
    private bool _cancelled;
    private Thread? _thread;

    internal TimerEffect(Action callback, int delayMs, bool repeat) {
        _callback = callback;
        _delayMs = delayMs;
        _repeat = repeat;
        _cancelled = false;
        _thread = null;
    }

    internal void Start() {
        _thread = new Thread(this.Run);
        _thread.IsBackground = true;
        _thread.Start();
    }

    private void Run() {
        while (!_cancelled) {
            int remaining = _delayMs;
            while (remaining > 0 && !_cancelled) {
                int chunk = remaining > 10 ? 10 : remaining;
                Thread.Sleep(chunk);
                remaining = remaining - chunk;
            }
            if (_cancelled) {
                return;
            }
            _callback();
            if (!_repeat) {
                return;
            }
        }
    }

    public void Dispose() {
        _cancelled = true;
        if (_thread != null) {
            // 限时回收：回调执行中则等待其完成（协作式取消不中断回调）
            _thread.Join(1000);
        }
    }
}
