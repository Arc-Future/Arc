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
    // 从「每帧无条件渲染」改为「仅需时渲染」：视觉变更经 Invalidate 标绘脏；
    // 几何/树结构变更经 InvalidateLayout 标布局脏（含绘脏）。
    // 帧泵：布局脏 → Measure/Arrange；绘脏或布局脏 → RenderFrame；随后 MarkRendered 清双旗。
    //
    // 空间脏矩形：API（InvalidateRegion）保留并升整窗 Invalidate。
    // **禁止**对 swapchain 纹理 LoadOp_Load 区域 Present——Fifo/Mailbox 每帧
    // 取得的 surface texture 与上一帧不是同一缓冲，Load 读到的是过期/未定义
    // 内容，再叠加根 scissor 只重画脏区 → 启动后黑屏闪烁（ArmlDemo 实证）。
    // 保留帧缓冲（offscreen）就绪前，区域 Present 一律回退整窗 Clear。
    // 布局脏 / 显式 Invalidate / resize / Motion 本就整窗 Clear+Present。

    /// <summary>是否需要重绘（纯视觉变更置 true；布局脏亦隐含绘脏）。</summary>
    private static bool _paintDirty = true;

    /// <summary>是否需要 Measure/Arrange（几何/树结构变更置 true）。</summary>
    private static bool _layoutDirty = true;

    /// <summary>本帧须整窗 Clear Present（swapchain 路径恒为 true）。</summary>
    private static bool _fullPaint = true;

    /// <summary>空间脏矩形（swapchain 停用区域 Present 后仅作合并占位，不驱动 LoadOp）。</summary>
    private static bool _hasRegion;

    private static double _dirtyX;
    private static double _dirtyY;
    private static double _dirtyW;
    private static double _dirtyH;

    /// <summary>标记一帧需要重绘（整窗 Present；幂等——一帧多变更合并一次 Present）。</summary>
    internal static void Invalidate() {
        _paintDirty = true;
        _fullPaint = true;
        _hasRegion = false;
    }

    /// <summary>标记需要全树 Measure/Arrange + 整窗重绘（几何/子树结构；隐含 Invalidate）。</summary>
    internal static void InvalidateLayout() {
        _layoutDirty = true;
        FramePump.Invalidate();
    }

    /// <summary>
    /// 标记 DIP 空间脏矩形。Swapchain 路径升整窗 Invalidate（见类首注释）；
    /// 宽高非正时同样升整窗。
    /// </summary>
    internal static void InvalidateRegion(double x, double y, double w, double h) {
        // swapchain 不可 LoadOp_Load 保留上一帧——区域脏一律整窗 Clear。
        FramePump.Invalidate();
    }

    /// <summary>强制下一 Present 整窗 Clear（resize / surface 重配 / Motion）。</summary>
    internal static void ForceFullPaint() {
        _fullPaint = true;
        _hasRegion = false;
    }

    /// <summary>本帧是否可走区域 Present——swapchain 路径恒 false（见 InvalidateRegion）。</summary>
    internal static bool HasPresentRegion() {
        return false;
    }

    internal static double PresentDirtyX() {
        return _dirtyX;
    }

    internal static double PresentDirtyY() {
        return _dirtyY;
    }

    internal static double PresentDirtyW() {
        return _dirtyW;
    }

    internal static double PresentDirtyH() {
        return _dirtyH;
    }

    /// <summary>当前是否需要渲染（绘脏或布局脏）。</summary>
    internal static bool NeedsRender() {
        return _paintDirty || _layoutDirty;
    }

    /// <summary>当前是否需要 Relayout（几何脏）。</summary>
    internal static bool NeedsLayout() {
        return _layoutDirty;
    }

    /// <summary>一次渲染完成：清绘脏与布局脏；允许后续 InvalidateRegion 走区域 Present。</summary>
    internal static void MarkRendered() {
        _paintDirty = false;
        _layoutDirty = false;
        _fullPaint = false;
        _hasRegion = false;
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

    /// <summary>距 caret 下一翻转的剩余毫秒（有焦点时空闲等待用；下限 16）。</summary>
    private static int CaretWaitMs() {
        if (!ImeBridge.HasFocusedInput()) {
            return 120;
        }
        long now = Stopwatch.GetTimestamp();
        long freq = Stopwatch.Frequency;
        if (freq <= 0 || _caretPhaseStartTick == 0) {
            return (int)CaretHalfPeriodMs;
        }
        double elapsedMs = (double)(now - _caretPhaseStartTick) * 1000.0 / (double)freq;
        double remain = CaretHalfPeriodMs - elapsedMs;
        if (remain < 16.0) {
            return 16;
        }
        if (remain > CaretHalfPeriodMs) {
            return (int)CaretHalfPeriodMs;
        }
        return (int)remain;
    }

    // ===== caret 闪烁相位机（RFC 026 M-caret · 桌面惯例：编辑重置相位、空闲 ~530ms 翻转）=====
    //
    // 墙钟驱动（禁按 WaitEvents 早醒次数计拍——消息唤醒会把「闪太快」放大到不可用）；
    // 键入/退格/caret 移动经 ResetCaretBlink 立即回「亮」相位。无焦点 TextBox 时
    // 相位恒亮、循环回到 -1 阻塞（零空转）。半周期对齐 Win32 默认 caret ~530ms。

    private static bool _caretOn = true;
    private static long _caretPhaseStartTick;
    private const double CaretHalfPeriodMs = 530.0;

    /// <summary>caret 当前相位（渲染端 caret 绘制条件之一）。</summary>
    internal static bool CaretBlinkOn() {
        return _caretOn;
    }

    /// <summary>编辑活动（键入/退格/caret 移动）：相位重置为亮。</summary>
    internal static void ResetCaretBlink() {
        _caretOn = true;
        _caretPhaseStartTick = Stopwatch.GetTimestamp();
    }

    /// <summary>墙钟推进：有焦点 TextBox 时按半周期翻转；可在 dirty/motion 帧调用。</summary>
    private static void PumpCaretIdle() {
        if (!ImeBridge.HasFocusedInput()) {
            _caretOn = true;
            _caretPhaseStartTick = 0;
            return;
        }
        long now = Stopwatch.GetTimestamp();
        if (_caretPhaseStartTick == 0) {
            _caretPhaseStartTick = now;
            return;
        }
        long freq = Stopwatch.Frequency;
        if (freq <= 0) {
            return;
        }
        double elapsedMs = (double)(now - _caretPhaseStartTick) * 1000.0 / (double)freq;
        if (elapsedMs < CaretHalfPeriodMs) {
            return;
        }
        int steps = (int)(elapsedMs / CaretHalfPeriodMs);
        if ((steps % 2) != 0) {
            _caretOn = !_caretOn;
        }
        _caretPhaseStartTick = now;
        // caret 翻转：优先脏焦点 TextBox 区域 Present；无焦点几何则升整窗纯绘。
        TextBox focus = ImeBridge.FocusedInput();
        if (focus != null) {
            double pad = 8.0;
            double w = focus.RenderWidth;
            double h = focus.RenderHeight;
            if (w <= 0.0) {
                w = InputMetrics.MinWidth;
            }
            if (h <= 0.0) {
                h = InputMetrics.MinHeight;
            }
            FramePump.InvalidateRegion(
                focus.LayoutX - pad,
                focus.LayoutY - pad,
                w + pad * 2.0,
                h + pad * 2.0);
        } else {
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
        // 不在此 Invalidate：调用方已在 dirty 路径；再标脏会与「渲染后无节流」叠成
        // 全速重绘空转（CodeEditor/DataGrid 页表现为卡死）。
        main.RelayoutSynced();
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
        FramePump.InvalidateLayout();
    }

    /// <summary>渲染一帧：BeginFrame → RenderElementTree → EndFrame。</summary>
    private static void RenderFrame(WgpuRender backend, long rootHandle,
                                    int width, int height) {
        if (backend == null) {
            return;
        }
        // Motion 插值可能跨控件——区域 Present 会漏绘，升整窗。
        if (MotionEngine.Active()) {
            FramePump.ForceFullPaint();
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
            // caret 墙钟：dirty/motion 帧也推进，避免焦点环插值期间「空 TextBox 不闪」。
            FramePump.PumpCaretIdle();
            // A-1②：布局脏 → Measure/Arrange；绘脏/布局脏 → Present；仅 Motion 时跳过布局。
            bool dirty = FramePump.NeedsRender();
            bool layout = FramePump.NeedsLayout();
            bool motion = MotionEngine.Active();
            if (dirty || motion) {
                if (layout) {
                    FramePump.RelayoutMainWindowAfterMetrics();
                }
                FramePump.RenderFrame(backend, rootHandle, width, height);
                FramePump.MarkRendered();
                // 任何重绘路径都必须节流：dirty 帧若零等待，会与 WM_PAINT/消息早醒
                // 叠成全速空转（第 8 tab CodeEditor+DataGrid 即卡死）。
                int paceMs = motion ? 15 : 16;
                if (ImeBridge.HasFocusedInput() && paceMs < 16) {
                    paceMs = 16;
                }
                WindowHost.WaitEvents(win, paceMs);
            } else {
                // 空闲：GIF 动画待切换帧 → 按到期剩余毫秒等待（不阻塞睡死）；
                // 有焦点 TextBox 时按 caret 半周期剩余等待；否则阻塞至新输入/唤醒。
                int animWait = FramePump.AnimationWaitMs();
                if (animWait >= 0) {
                    WindowHost.WaitEvents(win, animWait);
                } else if (ImeBridge.HasFocusedInput()) {
                    int caretWait = FramePump.CaretWaitMs();
                    WindowHost.WaitEvents(win, caretWait);
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
            FramePump.PumpCaretIdle();
            // A-1②：布局脏 → Measure/Arrange；绘脏/布局脏 → Present；仅 Motion 时跳过布局。
            bool dirty = FramePump.NeedsRender();
            bool layout = FramePump.NeedsLayout();
            bool motion = MotionEngine.Active();
            if (dirty || motion) {
                if (layout) {
                    FramePump.RelayoutMainWindowAfterMetrics();
                }
                FramePump.RenderFrame(backend, rootHandle, width, height);
                FramePump.MarkRendered();
            }
            // 异步骨架（M-AS1）：延迟节流替代忙让步——重绘/插值 ≥15ms；caret 墙钟剩余；
            // 否则 64ms 兜底；UiSynchronizationContext / EventLoop waker 合并后置 M-AS2。
            int animWait = FramePump.AnimationWaitMs();
            if (dirty || motion) {
                await Task.Delay(motion ? 15 : 16);
            } else if (animWait >= 0) {
                await Task.Delay(animWait);
            } else if (ImeBridge.HasFocusedInput()) {
                await Task.Delay(FramePump.CaretWaitMs());
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
