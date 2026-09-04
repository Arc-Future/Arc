// RFC 037 M-AS1 · RFC 037 Internal: frame pump skeleton (Input → posted work → EventPoll).
//
// M-AS1: PumpOnce drains UIDispatcher then rt_event_poll; no layout/draw phases yet.
// RunUntilClose is compat wrapper for blocking demos; RunAsync is non-blocking正道骨架.

namespace Arc.UI.Internal;

using Arc;
using Arc.Collections;
using Arc.Diagnostics;
using Arc.UI.Components;
using Arc.UI.Input;
using Arc.UI.Layout;
using Arc.UI.Rendering;
using Arc.UI.Rendering.Wgpu;

internal class FramePump {
    private FramePump() {
    }

    // ===== A-1 帧泵脏标记 + 按需渲染（RFC 037 §9.1 A-1②）=====
    //
    // 从「每帧无条件渲染」改为「仅需时渲染」：任何影响视觉的属性变更经
    // <see cref="Invalidate"/> 标记脏；帧泵循环仅在脏时执行一次 Measure/Arrange
    // + RenderFrame，随后 <see cref="MarkRendered"/> 清脏。
    // 初始 _dirty = true —— 首帧恒渲染（窗口刚创建必须上屏）。

    /// <summary>是否需要重绘（任何视觉变更置 true）。</summary>
    private static bool _dirty = true;

    /// <summary>标记一帧需要重绘（元素属性/树结构变更时调用；幂等——多次变更合并为一次渲染）。</summary>
    internal static void Invalidate() {
        _dirty = true;
    }

    /// <summary>当前是否需要渲染（帧泵循环据此按需渲染）。</summary>
    internal static bool NeedsRender() {
        return _dirty;
    }

    /// <summary>一次渲染完成：清脏（下次渲染须重新 Invalidate）。</summary>
    internal static void MarkRendered() {
        _dirty = false;
    }

    // ===== Image 动画保活（RFC 029 M2）=====
    //
    // Image 组件 ctor 自注册；每泵迭代 TickImages 推进 GIF 帧（延迟解码 + 帧上传 +
    // Invalidate 标脏请求重绘）。NextAnimationDue/AnimationWaitMs 决定空闲等待时长，
    // 避免「无输入阻塞 -1」把动画睡死。

    /// <summary>已注册 Image 组件（app 生命周期；元素移除不注销，与 Pointer/ScrollRouter 一致）。</summary>
    private static List<Image> _images = new List<Image>();

    /// <summary>Image 组件构造时自注册（内部入口，勿外部调用）。</summary>
    internal static void RegisterImage(Image image) {
        if (image == null) {
            return;
        }
        _images.Add(image);
    }

    /// <summary>每泵迭代：向所有已注册 Image 派发动画 tick（后端未就绪自动跳过）。</summary>
    private static void TickImages(WgpuRender backend) {
        int count = _images.Count;
        for (int i = 0; i < count; i++) {
            Image img = _images[i];
            if (img == null) {
                continue;
            }
            img.TickAnimation(backend);
        }
    }

    /// <summary>所有注册 Image 中最近到期的下一帧绝对时间戳（Stopwatch 域）；无动画返回 0。</summary>
    private static long NextAnimationDue() {
        long due = 0;
        int count = _images.Count;
        for (int i = 0; i < count; i++) {
            Image img = _images[i];
            if (img == null) {
                continue;
            }
            long d = img.NextFrameDueAt();
            if (d > 0 && (due == 0 || d < due)) {
                due = d;
            }
        }
        return due;
    }

    /// <summary>
    /// 空闲等待毫秒：有待切换 GIF 帧 → 距最近到期剩余毫秒（≥0，上限 1000 防病态长延时
    /// 睡死消息循环）；无动画返回 -1（由调用方走既有阻塞/节拍路径）。
    /// </summary>
    private static int AnimationWaitMs() {
        long due = FramePump.NextAnimationDue();
        if (due <= 0) {
            return -1;
        }
        long now = Stopwatch.GetTimestamp();
        long remain = due - now;
        if (remain <= 0) {
            return 0;
        }
        long ms = remain * 1000 / Stopwatch.Frequency;
        if (ms > 1000) {
            ms = 1000;
        }
        return (int)ms;
    }

    // ===== caret 闪烁相位机（RFC 026 M-caret · 桌面惯例：编辑重置相位、空闲 ~480ms 翻转）=====
    //
    // 空闲循环节拍 120ms（WaitEvents 超时），累计 4 拍（≈480ms）翻转相位并标脏；
    // 键入/退格/caret 移动经 ResetCaretBlink 立即回「亮」相位。无焦点 TextBox 时
    // 相位恒亮、循环回到 -1 阻塞（零空转）。

    private static bool _caretOn = true;
    private static int _caretIdleTicks;

    /// <summary>caret 当前相位（渲染端 caret 绘制条件之一）。</summary>
    internal static bool CaretBlinkOn() {
        return _caretOn;
    }

    /// <summary>编辑活动（键入/退格/caret 移动）：相位重置为亮。</summary>
    internal static void ResetCaretBlink() {
        _caretOn = true;
        _caretIdleTicks = 0;
    }

    /// <summary>空闲节拍推进：有焦点 TextBox 时 120ms 一拍，4 拍翻转。</summary>
    private static void PumpCaretIdle() {
        if (!ImeBridge.HasFocusedInput()) {
            _caretOn = true;
            _caretIdleTicks = 0;
            return;
        }
        _caretIdleTicks = _caretIdleTicks + 1;
        if (_caretIdleTicks >= 4) {
            _caretIdleTicks = 0;
            _caretOn = !_caretOn;
            FramePump.Invalidate();
        }
    }

    /// <summary>One pump tick: drain posted UI work, then poll native events.</summary>
    internal static void PumpOnce(long windowHandle) {
        UIDispatcher.DrainPostedWork();
        WindowHost.EventPoll(windowHandle);
    }

    /// <summary>Alias for PumpOnce (RFC 037 §6 IFramePump.Tick).</summary>
    internal static void Tick(long windowHandle) {
        FramePump.PumpOnce(windowHandle);
    }

    /// <summary>
    /// RFC 037 wgpu：初始化 WgpuRender 并绑定原生窗口。成功返回 true 并激活
    /// wgpu 接管渲染（WM_PAINT 跳过软件光栅）。
    /// </summary>
    private static WgpuRender InitWgpuRender(long win, int width, int height) {
        long hwnd = WindowHost.NativeHandle(win);
        if (hwnd == 0) {
            return null;
        }
        WgpuRender backend = new WgpuRender();
        bool ok = backend.Initialize(hwnd, (double)width, (double)height);
        if (!ok) {
            backend.Shutdown();
            return null;
        }
        WindowHost.SetWgpuActive(win, 1);
        // Flush 排队字体后再 Relayout：否则首次同源重测仍落到默认族（RegisterFamily 早于 HWND）。
        if (Application.Current != null) {
            Application.Current.Fonts.BindBackend(backend);
        }
        // Initialize 已 TextMeasuring.Attach；重测主窗体使布局与 DrawText 同源。
        FramePump.RelayoutMainWindowAfterMetrics();
        return backend;
    }

    /// <summary>度量服务挂接后：主窗体同源重布局（PrepareForShow 时可能尚无 atlas）。</summary>
    private static void RelayoutMainWindowAfterMetrics() {
        if (!TextMeasuring.IsAvailable()) {
            return;
        }
        if (Application.Current == null) {
            return;
        }
        Window main = Application.Current.MainWindow;
        if (main == null) {
            return;
        }
        main.RelayoutSynced();
        FramePump.Invalidate();
    }

    /// <summary>
    /// 轮询客户区物理尺寸 → DIP；若变化则写回 Window.Width/Height 并标脏重布局。
    /// GetClientSize 返回物理像素，与 CreateWindow / 命中测试同一 dpi 契约。
    /// </summary>
    private static void SyncClientSizeDip(long win, ref int width, ref int height) {
        int physW = 0;
        int physH = 0;
        WindowHost.GetClientSize(win, out physW, out physH);
        if (physW <= 0 || physH <= 0) {
            return;
        }
        double scale = WindowHost.SystemDpiScale();
        if (scale < 1.0) {
            scale = 1.0;
        }
        int dipW = (int)((double)physW / scale);
        int dipH = (int)((double)physH / scale);
        if (dipW < 1) {
            dipW = 1;
        }
        if (dipH < 1) {
            dipH = 1;
        }
        if (dipW == width && dipH == height) {
            return;
        }
        width = dipW;
        height = dipH;
        if (Application.Current != null) {
            Window main = Application.Current.MainWindow;
            if (main != null) {
                main.Width = (double)dipW;
                main.Height = (double)dipH;
            }
        }
        FramePump.Invalidate();
    }

    /// <summary>渲染一帧：BeginFrame → RenderElementTree → EndFrame。</summary>
    private static void RenderFrame(WgpuRender backend, long rootHandle,
                                    int width, int height) {
        if (backend == null) {
            return;
        }
        backend.BeginFrame((double)width, (double)height);
        backend.RenderElementTree(rootHandle);
        backend.EndFrame();
    }

    /// <summary>
    /// Blocking message loop using PumpOnce (compat entry — not终态正道; see Application.RunAsync).
    /// </summary>
    internal static void RunUntilClose(string title, int width, int height, long rootHandle) {
        long win = WindowHost.CreateWindow(title, width, height);
        if (win == (long)0) {
            return;
        }
        WindowHost.SetRootElement(win, rootHandle);
        // RFC 037 wgpu：初始化渲染后端并接管渲染；失败则回退窗口空转（无渲染）。
        // 注意：CreateWindow内部已通过AdjustWindowRectEx将width/height（客户区）转换为窗口外尺寸，
        // 因此这里直接传入期望的客户区width/height给wgpu后端即可。
        // BindBackend 已在 InitWgpuRender 内、Relayout 之前完成（同源度量）。
        WgpuRender backend = FramePump.InitWgpuRender(win, width, height);
        FocusManager.SetWindowHandle(win);
        FocusManager.ActivateInitialFocus();
        ImeBridge.ActivateDefaultFocus();
        UIDispatcher.Reset();
        UIDispatcher.MarkUIThread();
        while (WindowHost.ShouldClose(win) == 0) {
            FramePump.PumpOnce(win);
            FramePump.SyncClientSizeDip(win, ref width, ref height);
            // Image 动画：GIF 帧推进（延迟解码 + 帧上传 + 标脏）；须在 NeedsRender 之前。
            FramePump.TickImages(backend);
            // A-1②：脏 → Measure/Arrange + 渲染；仅 Motion 插值时跳过布局（几何未变）。
            bool dirty = FramePump.NeedsRender();
            bool motion = MotionEngine.Active();
            if (dirty || motion) {
                if (dirty) {
                    FramePump.RelayoutMainWindowAfterMetrics();
                }
                FramePump.RenderFrame(backend, rootHandle, width, height);
                FramePump.MarkRendered();
                if (motion && !dirty) {
                    // 纯插值帧：15ms（≈60fps）节拍唤醒，避免全速重绘空转。
                    WindowHost.WaitEvents(win, 15);
                }
            } else {
                // 空闲：GIF 动画待切换帧 → 按到期剩余毫秒等待（不阻塞睡死）；
                // 有焦点 TextBox 时 120ms 节拍推进 caret 闪烁；否则阻塞至
                // 新输入/唤醒消息（跨线程 Post 经 WakeUIThread 唤醒）。
                int animWait = FramePump.AnimationWaitMs();
                if (animWait >= 0) {
                    WindowHost.WaitEvents(win, animWait);
                } else if (ImeBridge.HasFocusedInput()) {
                    WindowHost.WaitEvents(win, 120);
                    FramePump.PumpCaretIdle();
                } else {
                    WindowHost.WaitEvents(win, -1);
                }
            }
        }
        if (backend != null) {
            if (Application.Current != null)
            {
                Application.Current.Fonts.UnbindBackend();
            }
            WindowHost.SetWgpuActive(win, 0);
            backend.Shutdown();
        }
        UIDispatcher.ClearUIThread();
        WindowHost.DestroyWindow(win);
    }

    /// <summary>
    /// Non-blocking pump skeleton (M-AS1): yields between PumpOnce iterations.
    /// UiSynchronizationContext / EventLoop waker merge deferred to M-AS2.
    /// </summary>
    internal static async Task RunAsync(string title, int width, int height, long rootHandle) {
        long win = WindowHost.CreateWindow(title, width, height);
        if (win == (long)0) {
            return;
        }
        WindowHost.SetRootElement(win, rootHandle);
        // BindBackend 已在 InitWgpuRender 内、Relayout 之前完成（同源度量）。
        WgpuRender backend = FramePump.InitWgpuRender(win, width, height);
        FocusManager.SetWindowHandle(win);
        FocusManager.ActivateInitialFocus();
        ImeBridge.ActivateDefaultFocus();
        UIDispatcher.Reset();
        UIDispatcher.MarkUIThread();
        while (WindowHost.ShouldClose(win) == 0) {
            FramePump.PumpOnce(win);
            FramePump.SyncClientSizeDip(win, ref width, ref height);
            // Image 动画：GIF 帧推进（延迟解码 + 帧上传 + 标脏）；须在 NeedsRender 之前。
            FramePump.TickImages(backend);
            // A-1②：脏 → Measure/Arrange + 渲染；仅 Motion 插值时跳过布局（几何未变）。
            bool dirty = FramePump.NeedsRender();
            bool motion = MotionEngine.Active();
            if (dirty || motion) {
                if (dirty) {
                    FramePump.RelayoutMainWindowAfterMetrics();
                }
                FramePump.RenderFrame(backend, rootHandle, width, height);
                FramePump.MarkRendered();
            }
            // 异步骨架（M-AS1）：延迟节流替代忙让步——纯插值帧 15ms（≈60fps）；
            // GIF 动画待切换帧按其到期剩余毫秒节拍（不睡死）；
            // 空闲且有焦点 TextBox 时 120ms 节拍推进 caret 闪烁，否则 64ms 兜底轮询；
            // UiSynchronizationContext / EventLoop waker 合并后置 M-AS2。
            int animWait = FramePump.AnimationWaitMs();
            if (motion) {
                await Task.Delay(15);
            } else if (animWait >= 0) {
                await Task.Delay(animWait);
            } else if (ImeBridge.HasFocusedInput()) {
                await Task.Delay(120);
                FramePump.PumpCaretIdle();
            } else {
                await Task.Delay(64);
            }
        }
        if (backend != null) {
            if (Application.Current != null)
            {
                Application.Current.Fonts.UnbindBackend();
            }
            WindowHost.SetWgpuActive(win, 0);
            backend.Shutdown();
        }
        UIDispatcher.ClearUIThread();
        WindowHost.DestroyWindow(win);
    }
}
