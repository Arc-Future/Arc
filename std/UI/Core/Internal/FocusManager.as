// RFC 037 §8 M-focus2：Tab / Shift+Tab 焦点循环 + 键盘激活。
//
// 动态容量注册表（List）：可聚焦控件数不受固定 8 槽静默上限约束；
// SoftCapacity=64 起溢出告警（P3 无静默丢弃），仍继续登记。
// Enter/Space → InputElement.Activate（Button→Click、ToggleButton→Toggle）。
//
// **IsFocusVisible**：键盘导航（Tab/方向）置 true；指针聚焦置 false。
// 焦点环仅在 IsFocusVisible 时绘制（对标 :focus-visible）；IsFocused 仍驱动 caret/VSM。

namespace Arc.UI.Internal;

using Arc.Collections;
using Arc.Diagnostics;
using Arc.UI;
using Arc.UI.Components;
using Arc.UI.Components.Primitives;
using Arc.UI.Input;

internal class FocusManager {
    /// <summary>软容量：超出后告警但仍登记（RFC 037 §8 ≥64 + 溢出告警）。</summary>
    const int SoftCapacity = 64;

    static List<long> _handles = new List<long>();
    static List<Control> _controls = new List<Control>();
    static int _focusIndex;
    static long _windowHandle;
    static int _installed;
    /// <summary>是否显示焦点环（键盘模态；指针聚焦清除）。</summary>
    static bool _focusVisible;

    private FocusManager() {
    }

    // Win32 虚拟键码（RFC 037 M-focus M5 方向导航 + InputElement.OnKeyDown 消费集）。
    internal static int VirtualKeyTab() { return 9; }
    internal static int VirtualKeyReturn() { return 13; }
    /// <summary>Win32 VK_ESCAPE — Popup 轻关闭 / 无弹层时退主窗。</summary>
    internal static int VirtualKeyEscape() { return 27; }
    internal static int VirtualKeySpace() { return 32; }
    internal static int VirtualKeyLeft() { return 37; }
    internal static int VirtualKeyUp() { return 38; }
    internal static int VirtualKeyRight() { return 39; }
    internal static int VirtualKeyDown() { return 40; }
    internal static int VirtualKeyHome() { return 36; }
    internal static int VirtualKeyEnd() { return 35; }
    internal static int VirtualKeyBackspace() { return 8; }
    internal static int VirtualKeyDelete() { return 46; }

    /// <summary>当前是否应绘制焦点环（键盘导航后为 true）。</summary>
    internal static bool IsFocusVisible() {
        return _focusVisible;
    }

    internal static void Reset() {
        _handles.Clear();
        _controls.Clear();
        _focusIndex = -1;
        _windowHandle = 0;
        _installed = 0;
        _focusVisible = false;
    }

    internal static void RegisterTabStop(Control ctrl, long platformHandle) {
        if (ctrl == null || platformHandle == 0) {
            return;
        }
        if (!ctrl.Focusable || !ctrl.IsTabStop || !ctrl.IsEnabled) {
            return;
        }
        int count = _handles.Count;
        if (count >= SoftCapacity) {
            Console.WriteLine("[FocusManager] tab registry soft capacity exceeded ("
                + SoftCapacity + "); registering anyway (count=" + count + ")");
        }
        _handles.Add(platformHandle);
        _controls.Add(ctrl);
    }

    internal static void SetWindowHandle(long windowHandle) {
        _windowHandle = windowHandle;
    }

    internal static void Install() {
        if (_installed != 0) {
            return;
        }
        // IN-R2：键盘入口归 KeyboardRouter（key+text）；本类仅承接导航。
        KeyboardRouter.Install();
        _installed = 1;
    }

    internal static void ActivateInitialFocus() {
        // 启动焦点不显示环——等首次 Tab/方向键才置 IsFocusVisible。
        _focusVisible = false;
        if (_handles.Count == 0) {
            _focusIndex = -1;
            return;
        }
        // 优先首个 TextBox，使启动 caret/IME 与 Tab 环一致。
        int i = 0;
        int count = _handles.Count;
        while (i < count) {
            Control c = ControlAt(i);
            if (c != null && (c.TypeName == "TextBox" || c.TypeName == "PasswordBox")) {
                FocusManager.SetFocusIndex(i);
                return;
            }
            i = i + 1;
        }
        FocusManager.SetFocusIndex(0);
    }

    static long HandleAt(int idx) {
        return _handles[idx];
    }

    static Control ControlAt(int idx) {
        return _controls[idx];
    }

    static int TabCount() {
        return _handles.Count;
    }

    /// <summary>
    /// 焦点视觉状态单一写点：InputElement 经 SetFocused → OnFocusedChanged
    /// （状态 + 镜像 + 标脏单一链）；非输入 Control 兜底直写镜像。
    /// </summary>
    static void ApplyFocused(Control ctrl, long handle, bool focused) {
        if (ctrl is InputElement) {
            InputElement el = (InputElement)ctrl;
            el.SetFocused(focused);
            return;
        }
        WindowHost.ElementSetBool(handle, "IsFocused", focused ? 1 : 0);
        int vis = 0;
        if (focused) {
            if (_focusVisible) {
                vis = 1;
            }
        }
        WindowHost.ElementSetBool(handle, "IsFocusVisible", vis);
        FramePump.Invalidate();
    }

    static void SetFocusIndex(int idx) {
        int count = FocusManager.TabCount();
        if (count == 0) {
            return;
        }
        if (idx < 0) {
            idx = count - 1;
        }
        if (idx >= count) {
            idx = 0;
        }

        int prev = _focusIndex;
        if (prev >= 0 && prev < count) {
            FocusManager.ApplyFocused(ControlAt(prev), HandleAt(prev), false);
        }
        _focusIndex = idx;
        FocusManager.ApplyFocused(ControlAt(idx), HandleAt(idx), true);

        Control ctrl = ControlAt(idx);
        ImeBridge.ClearFocused();
        if (ctrl is InputElement) {
            InputElement el = (InputElement)ctrl;
            el.OnGotFocus();
        }

        if (_windowHandle != 0) {
            WindowHost.InvalidateActiveWindow();
        }
        FramePump.Invalidate();
    }

    /// <summary>按平台句柄设置焦点（点击 TextBox / 外部同步 Tab 索引）。指针路径：清 IsFocusVisible。</summary>
    internal static bool FocusPlatformHandle(long platformHandle) {
        if (platformHandle == 0) {
            return false;
        }
        int count = FocusManager.TabCount();
        int i = 0;
        while (i < count) {
            if (FocusManager.HandleAt(i) == platformHandle) {
                _focusVisible = false;
                FocusManager.SetFocusIndex(i);
                return true;
            }
            i = i + 1;
        }
        return false;
    }

    /// <summary>
    /// 焦点导航键分发（KeyboardRouter 未消费编辑键后调用）。
    /// TextBox 编辑已由 TextBoxController.HandleKey 处理；此处仅 Tab/
    /// 方向导航与 Enter/Space 激活。
    /// </summary>
    internal static void RouteKey(int virtualKey, int shiftDown) {
        int tabCount = FocusManager.TabCount();
        if (tabCount == 0) {
            return;
        }
        // 非 TextBox 的 InputElement 仍可经 OnKeyDown 消费（Button 等）。
        if (_focusIndex >= 0 && _focusIndex < tabCount) {
            Control focused = ControlAt(_focusIndex);
            if (focused is Selector) {
                Selector selector = (Selector)focused;
                if (selector.TryHandleKey(virtualKey)) {
                    _focusVisible = true;
                    return;
                }
            }
            if (focused is InputElement && !(focused is TextBox)) {
                InputElement el = (InputElement)focused;
                if (el.OnKeyDown(virtualKey, shiftDown)) {
                    return;
                }
            }
        }
        if (virtualKey == FocusManager.VirtualKeyTab()) {
            int delta = shiftDown != 0 ? -1 : 1;
            int next = _focusIndex + delta;
            if (_focusIndex < 0) {
                next = shiftDown != 0 ? tabCount - 1 : 0;
            }
            _focusVisible = true;
            FocusManager.SetFocusIndex(next);
            return;
        }
        // M5 方向导航（RFC 037 M5）：方向键沿 Tab 循环顺序移动焦点。
        int directionDelta = 0;
        if (virtualKey == FocusManager.VirtualKeyLeft() ||
            virtualKey == FocusManager.VirtualKeyUp()) {
            directionDelta = -1;
        } else if (virtualKey == FocusManager.VirtualKeyRight() ||
                   virtualKey == FocusManager.VirtualKeyDown()) {
            directionDelta = 1;
        }
        if (directionDelta != 0) {
            int next = _focusIndex + directionDelta;
            if (_focusIndex < 0) {
                next = directionDelta > 0 ? 0 : tabCount - 1;
            }
            _focusVisible = true;
            FocusManager.SetFocusIndex(next);
            return;
        }
        if (virtualKey == FocusManager.VirtualKeyReturn() ||
            virtualKey == FocusManager.VirtualKeySpace()) {
            if (_focusIndex < 0 || _focusIndex >= tabCount) {
                return;
            }
            // Enter/Space 默认激活（InputElement.Activate）：Button→Click、
            // ToggleButton/CheckBox→Toggle（WPF ButtonBase 同构）。
            Control ctrl = ControlAt(_focusIndex);
            if (ctrl is InputElement) {
                InputElement el = (InputElement)ctrl;
                el.Activate();
            }
        }
    }
}
