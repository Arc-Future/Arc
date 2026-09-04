// RFC 037 · 026-ime-east-asian-input.md §5.2（handler 推送 ABI）
//
// 平台 WM_IME_* → rt_ui_ime_set_handler 回调 → 本类 OnNativeEvent
// 转发至 focused TextBox（COMMIT / COMPOSITION / FOCUS_LOST）。
//
// RFC 037 §8 修订（text-editing.md）：平台 Kind 只做机械转换，编辑语义
// 全部经 TextBoxController → TextBoxModel 内核裁决（D6 消除）。本类待
// IN-R2 收缩为 IME 专用桥（KeyboardRouter 单一键盘通道接管编辑键），
// 焦点跟随 FocusManager 单向，见 docs/plan.md「UI 输入栈重构」。

namespace Arc.UI.Input;

using Arc.UI.Components;
using Arc.UI.Internal;

/// <summary>IME handler 桥：注册全局回调并维护 focused TextBox。</summary>
internal class ImeBridge {
    static TextBox _focused;
    static TextBox _firstInput;

    public static int KindCompositionUpdate() { return 1; }
    public static int KindCommit() { return 2; }
    public static int KindCompositionEnd() { return 3; }
    public static int KindFocusLost() { return 4; }
    public static int KindBackspace() { return 5; }
    public static int KindAsciiChar() { return 6; }
    public static int KindCaretLeft() { return 7; }
    public static int KindCaretRight() { return 8; }
    public static int KindCaretLeftExtend() { return 9; }
    public static int KindCaretRightExtend() { return 10; }
    public static int KindSelectAll() { return 11; }
    public static int KindDeleteForward() { return 12; }
    public static int KindCaretHome() { return 13; }
    public static int KindCaretEnd() { return 14; }
    public static int KindCaretHomeExtend() { return 15; }
    public static int KindCaretEndExtend() { return 16; }

    /// <summary>Application.Run 启动时注册 rt_ui_ime_set_handler。</summary>
    public static void InstallHandler() {
        WindowHost.ImeInstallHandler();
        WindowHost.SetInputClickHandler(ImeBridge.RouteInputClick);
        ImeBridge.WarmupHandler();
    }

    /// <summary>平台点击 TextBox（pointer_win32 M-caret2）：局部 DIP 坐标定位 caret。</summary>
    internal static void RouteInputClick(long handle, double localDipX) {
        TextBox focus = _focused;
        if (focus == null || focus.MirrorHandle() != handle) {
            return;
        }
        TextBoxController.HandleClick(focus, localDipX);
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
        } else if (kind == KindBackspace()) {
            TextBoxController.HandleBackspace(focus);
        } else if (kind == KindAsciiChar()) {
            string ch = WindowHost.NativeCStringFromPtr(payloadPtr);
            TextBoxController.HandleAscii(focus, ch);
        } else if (kind == KindCaretLeft()) {
            TextBoxController.HandleCaretChar(focus, true, false);
        } else if (kind == KindCaretRight()) {
            TextBoxController.HandleCaretChar(focus, false, false);
        } else if (kind == KindCaretLeftExtend()) {
            TextBoxController.HandleCaretChar(focus, true, true);
        } else if (kind == KindCaretRightExtend()) {
            TextBoxController.HandleCaretChar(focus, false, true);
        } else if (kind == KindSelectAll()) {
            TextBoxController.HandleSelectAll(focus);
        } else if (kind == KindDeleteForward()) {
            TextBoxController.HandleDelete(focus);
        } else if (kind == KindCaretHome()) {
            TextBoxController.HandleHome(focus, false);
        } else if (kind == KindCaretEnd()) {
            TextBoxController.HandleEnd(focus, false);
        } else if (kind == KindCaretHomeExtend()) {
            TextBoxController.HandleHome(focus, true);
        } else if (kind == KindCaretEndExtend()) {
            TextBoxController.HandleEnd(focus, true);
        }
    }

    /// <summary>注册可聚焦 TextBox；首个注册项为默认焦点（M-ime1）。</summary>
    public static void RegisterInput(TextBox input) {
        if (input == null) {
            return;
        }
        if (_firstInput == null) {
            _firstInput = input;
        }
        if (_focused == null) {
            ImeBridge.SetFocused(input);
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
    /// 布局完成后激活默认 IME 焦点：若 Tab 初始焦点落在非 TextBox 控件
    /// （ActivateInitialFocus 经 ClearFocused 清空 IME 焦点），回退激活
    /// 首个注册 TextBox——保证启动即可见 caret、空闲循环推进闪烁节拍。
    /// </summary>
    public static void ActivateDefaultFocus() {
        if (_focused == null && _firstInput != null) {
            ImeBridge.SetFocused(_firstInput);
            return;
        }
        TextBox focus = _focused;
        if (focus != null) {
            focus.ApplyImeFocus();
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
