// RFC 037 · 026-ime-east-asian-input.md §5.2（handler 推送 ABI）
//
// 平台 WM_IME_* → rt_ui_ime_set_handler 回调 → 本类 OnNativeEvent
// 转发至 focused TextBox（COMMIT / COMPOSITION / FOCUS_LOST）。
//
// RFC 037 §8 IN-R2：本类收缩为 IME 专用桥（不含编辑命令 kind）；
// 编辑键/可打印字符经 KeyboardRouter（rt_ui_dispatch_key/text）→
// TextBoxController。焦点跟随 FocusManager 单向。

namespace Arc.UI.Input;

using Arc.Diagnostics;
using Arc.UI.Components;
using Arc.UI.Internal;

/// <summary>IME handler 桥：组字/commit/失焦；编辑键不经此通道。</summary>
internal class ImeBridge {
    static TextBox _focused;
    static TextBox _firstInput;

    /// <summary>双击判定：同句柄、时限内、DIP 位移容差（对标系统双击，无平台 ABI）。</summary>
    const int DoubleClickMs = 500;
    const double DoubleClickSlopDip = 4.0;
    static long _lastClickTicks;
    static long _lastClickHandle;
    static double _lastClickDipX;

    public static int KindCompositionUpdate() { return 1; }
    public static int KindCommit() { return 2; }
    public static int KindCompositionEnd() { return 3; }
    public static int KindFocusLost() { return 4; }

    /// <summary>Application.Run 启动时注册 rt_ui_ime_set_handler。</summary>
    public static void InstallHandler() {
        WindowHost.ImeInstallHandler();
        WindowHost.SetInputClickHandler(ImeBridge.RouteInputClick);
        ImeBridge.WarmupHandler();
    }

    /// <summary>平台点击 TextBox（pointer_win32）：先确保焦点，再局部 DIP 定位 caret / 双击词选。</summary>
    internal static void RouteInputClick(long handle, double localDipX) {
        FocusManager.FocusPlatformHandle(handle);
        TextBox focus = _focused;
        if (focus == null || focus.MirrorHandle() != handle) {
            return;
        }
        long now = Stopwatch.GetTimestamp();
        bool isDouble = false;
        if (handle == _lastClickHandle && _lastClickTicks > 0) {
            long elapsedMs = (now - _lastClickTicks) * 1000 / Stopwatch.Frequency;
            double dx = localDipX - _lastClickDipX;
            if (dx < 0.0) {
                dx = 0.0 - dx;
            }
            if (elapsedMs >= 0 && elapsedMs <= DoubleClickMs && dx <= DoubleClickSlopDip) {
                isDouble = true;
            }
        }
        _lastClickTicks = now;
        _lastClickHandle = handle;
        _lastClickDipX = localDipX;
        if (isDouble) {
            // 消费本对，避免三击连触发第二次词选。
            _lastClickTicks = 0;
            TextBoxController.HandleDoubleClick(focus, localDipX);
        } else {
            TextBoxController.HandleClick(focus, localDipX);
        }
    }

    /// <summary>平台拖拽 TextBox（按下后移动）：固定 anchor，扩展选区活动端。</summary>
    internal static void RouteInputDrag(long handle, double localDipX) {
        TextBox focus = _focused;
        if (focus == null || focus.MirrorHandle() != handle) {
            return;
        }
        TextBoxController.HandleDrag(focus, localDipX);
    }

    /// <summary>保留 OnNativeEvent 符号供 C 链接；运行时 _focused 为空时为 no-op。</summary>
    public static void WarmupHandler() {
        ImeBridge.OnNativeEvent((long)0, (long)0, KindFocusLost(), (long)0);
    }

    /// <summary>平台 IME 回调（codegen → Arc_UI_Input_ImeBridge_OnNativeEvent）。</summary>
    public static void OnNativeEvent(long ctx, long targetHandle, int kind, long payloadPtr) {
        TextBox focus = _focused;
        if (focus == null) {
            return;
        }
        if (kind == KindCommit()) {
            string chunk = WindowHost.NativeCStringFromPtr(payloadPtr);
            TextBoxController.HandleCommit(focus, chunk);
        } else if (kind == KindCompositionUpdate()) {
            string comp = WindowHost.ImeCompositionText(payloadPtr);
            TextBoxController.HandleComposition(focus, comp);
        } else if (kind == KindCompositionEnd()) {
            TextBoxController.HandleComposition(focus, "");
        } else if (kind == KindFocusLost()) {
            focus.SetFocused(false);
            _focused = null;
        }
    }

    /// <summary>当前 IME/键盘编辑焦点 TextBox（KeyboardRouter 分发依据）。</summary>
    internal static TextBox FocusedInput() {
        return _focused;
    }

    /// <summary>注册可聚焦 TextBox；首个登记项记录，建树期不抢 FocusManager 写点。</summary>
    public static void RegisterInput(TextBox input) {
        if (input == null) {
            return;
        }
        if (_firstInput == null) {
            _firstInput = input;
        }
    }

    /// <summary>是否存在持有 IME 焦点的 TextBox（FramePump caret 闪烁节拍依据）。</summary>
    internal static bool HasFocusedInput() {
        return _focused != null;
    }

    /// <summary>切换 IME 焦点至指定 TextBox。</summary>
    public static void ClearFocused() {
        TextBox prev = _focused;
        if (prev != null) {
            prev.SetFocused(false);
        }
        _focused = null;
        WindowHost.ImeSetFocus((long)0);
    }

    public static void SetFocused(TextBox input) {
        TextBox prev = _focused;
        if (prev != null && prev != input) {
            prev.SetFocused(false);
        }
        _focused = input;
        if (input == null) {
            WindowHost.ImeSetFocus((long)0);
            return;
        }
        input.SetFocused(true);
        input.ApplyImeFocus();
    }

    /// <summary>
    /// 布局完成后同步 IME：已有 IME 焦点则刷新候选窗；否则经 FocusManager
    /// 聚焦首个已登记 TextBox（禁旁路 SetFocused 双写 IsFocused）。
    /// </summary>
    public static void ActivateDefaultFocus() {
        if (_focused != null) {
            _focused.ApplyImeFocus();
            return;
        }
        if (_firstInput == null) {
            return;
        }
        long h = _firstInput.MirrorHandle();
        if (h != 0) {
            FocusManager.FocusPlatformHandle(h);
        }
    }

    /// <summary>
    /// 带主动刷新的软件渲染消息循环（handler 在 EventPoll 内同步触发）。
    /// M-AS1：委托 FramePump.RunUntilClose（PumpOnce 内核）；阻塞 compat 入口，非终态正道。
    /// </summary>
    public static void RunMessageLoop(string title, int width, int height, long rootHandle) {
        FramePump.RunUntilClose(title, width, height, rootHandle);
    }
}
