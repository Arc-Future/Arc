// RFC 037 M-AS1 · RFC 037 Internal: UI thread dispatcher skeleton.
//
// Post / InvokeAsync are narrow escape hatches for background → UI marshaling.
// Not a WPF Dispatcher clone; not user fine-control API (RFC 037 §2.1).
//
// M-AS1 honesty: fixed-slot queue (≤8 pending Post); no EventLoop waker merge yet;
// InvokeAsync does not await completion when posted from off-UI thread (M-AS2).

namespace Arc.UI.Internal;

using Arc;
using Arc.UI.Components;

/// <summary>UI work priority (M-AS1 subset; not WPF 10-level DispatcherPriority).</summary>
internal enum UIPriority {
    Idle = 0,
    Loaded = 1,
    Normal = 2,
    Input = 3,
    Render = 4,
    Send = 5,
}

/// <summary>
/// UI-thread work queue skeleton. Framework wiring only — application authors
/// use <c>Application.RunAsync</c> and <c>await OpenPathAsync</c> (RFC 037), not
/// manual Invoke (RFC 037 §1 WPF anti-pattern table).
/// </summary>
internal class UIDispatcher {
    static int _uiThreadActive;
    static int _postCount;
    static Action _work0;
    static Action _work1;
    static Action _work2;
    static Action _work3;
    static Action _work4;
    static Action _work5;
    static Action _work6;
    static Action _work7;

    private UIDispatcher() {
    }

    internal static void Reset() {
        _uiThreadActive = 0;
        _postCount = 0;
        _work0 = null;
        _work1 = null;
        _work2 = null;
        _work3 = null;
        _work4 = null;
        _work5 = null;
        _work6 = null;
        _work7 = null;
    }

    /// <summary>True when the current call stack is executing on the UI pump thread.</summary>
    internal static bool CheckAccess() {
        return _uiThreadActive != 0;
    }

    internal static void MarkUIThread() {
        _uiThreadActive = 1;
    }

    internal static void ClearUIThread() {
        _uiThreadActive = 0;
    }

    /// <summary>
    /// Enqueue work for the next <see cref="FramePump.PumpOnce"/> drain on the UI thread.
    /// Long I/O / CPU paths must use this instead of blocking inside EventPoll (RFC 037 §2.2).
    /// </summary>
    internal static void Post(UIPriority priority, Action work) {
        if (work == null) {
            return;
        }
        if (_postCount >= 8) {
            return;
        }
        if (_postCount == 0) {
            _work0 = work;
        } else if (_postCount == 1) {
            _work1 = work;
        } else if (_postCount == 2) {
            _work2 = work;
        } else if (_postCount == 3) {
            _work3 = work;
        } else if (_postCount == 4) {
            _work4 = work;
        } else if (_postCount == 5) {
            _work5 = work;
        } else if (_postCount == 6) {
            _work6 = work;
        } else if (_postCount == 7) {
            _work7 = work;
        }
        _postCount = _postCount + 1;
        // 跨线程入队后唤醒空闲阻塞中的帧泵（WaitEvents），使 DrainPostedWork 立即执行。
        if (!UIDispatcher.CheckAccess()) {
            WindowHost.WakeUIThread();
        }
    }

    /// <summary>Drain all posted work (called from FramePump before EventPoll).</summary>
    internal static void DrainPostedWork() {
        int count = _postCount;
        _postCount = 0;
        if (count > 0) {
            Action w0 = _work0;
            _work0 = null;
            if (w0 != null) {
                w0();
            }
        }
        if (count > 1) {
            Action w1 = _work1;
            _work1 = null;
            if (w1 != null) {
                w1();
            }
        }
        if (count > 2) {
            Action w2 = _work2;
            _work2 = null;
            if (w2 != null) {
                w2();
            }
        }
        if (count > 3) {
            Action w3 = _work3;
            _work3 = null;
            if (w3 != null) {
                w3();
            }
        }
        if (count > 4) {
            Action w4 = _work4;
            _work4 = null;
            if (w4 != null) {
                w4();
            }
        }
        if (count > 5) {
            Action w5 = _work5;
            _work5 = null;
            if (w5 != null) {
                w5();
            }
        }
        if (count > 6) {
            Action w6 = _work6;
            _work6 = null;
            if (w6 != null) {
                w6();
            }
        }
        if (count > 7) {
            Action w7 = _work7;
            _work7 = null;
            if (w7 != null) {
                w7();
            }
        }
    }

    /// <summary>
    /// Run work on the UI thread. On-UI: synchronous. Off-UI: Post + CompletedTask (M-AS1 skeleton).
    /// </summary>
    internal static Task InvokeAsync(Action work) {
        if (work == null) {
            return Task.CompletedTask;
        }
        if (UIDispatcher.CheckAccess()) {
            work();
            return Task.CompletedTask;
        }
        UIDispatcher.Post(UIPriority.Send, work);
        return Task.CompletedTask;
    }

    /// <summary>Func variant of InvokeAsync (M-AS1: result available only on CheckAccess path).</summary>
    internal static Task<T> InvokeAsync<T>(Func<T> work) {
        if (work == null) {
            return Task<T>.FromResult(default(T));
        }
        if (UIDispatcher.CheckAccess()) {
            T value = work();
            return Task<T>.FromResult(value);
        }
        UIDispatcher.Post(UIPriority.Send, () => { work(); });
        return Task<T>.FromResult(default(T));
    }
}
