// RFC 037 M-focus Draft · RFC 037 Internal: Tab / Shift+Tab focus cycle + keyboard activation.
//
// Draft: fixed-slot tab registry (List<long> monomorphization not ready for Show).
//
// 【已知根因 · RFC 037 §8 修订（M-focus2）】固定 8 槽注册表与 InputFocusRouter 的 8 槽
// slot 在可聚焦控件 >8 时静默丢弃注册（ArmlDemo「点击 TextBox 无 caret、无法输入」根因）——
// 待 IN-R3 重构：动态容量 + SetFocusedControl 唯一焦点出口 + ImeBridge 单向跟随，
// 契约见 docs/rfc/037-ui.md §8，任务表见 docs/plan.md「UI 输入栈重构」。

namespace Arc.UI.Internal;

using Arc.UI;
using Arc.UI.Components;
using Arc.UI.Input;

internal class FocusManager {
    static int _tabCount = 0;
    static long _handle0 = 0;
    static long _handle1 = 0;
    static long _handle2 = 0;
    static long _handle3 = 0;
    static long _handle4 = 0;
    static long _handle5 = 0;
    static long _handle6 = 0;
    static long _handle7 = 0;
    static Control _ctrl0 = null;
    static Control _ctrl1 = null;
    static Control _ctrl2 = null;
    static Control _ctrl3 = null;
    static Control _ctrl4 = null;
    static Control _ctrl5 = null;
    static Control _ctrl6 = null;
    static Control _ctrl7 = null;
    static int _focusIndex;
    static long _windowHandle;
    static int _installed;

    private FocusManager() {
    }

    // Win32 虚拟键码（RFC 037 M-focus M5 方向导航 + InputElement.OnKeyDown 消费集）。
    internal static int VirtualKeyTab() { return 9; }
    internal static int VirtualKeyReturn() { return 13; }
    internal static int VirtualKeySpace() { return 32; }
    internal static int VirtualKeyLeft() { return 37; }
    internal static int VirtualKeyUp() { return 38; }
    internal static int VirtualKeyRight() { return 39; }
    internal static int VirtualKeyDown() { return 40; }
    internal static int VirtualKeyHome() { return 36; }
    internal static int VirtualKeyEnd() { return 35; }
    internal static int VirtualKeyBackspace() { return 8; }
    internal static int VirtualKeyDelete() { return 46; }

    internal static void Reset() {
        _tabCount = 0;
        _handle0 = 0;
        _handle1 = 0;
        _handle2 = 0;
        _handle3 = 0;
        _handle4 = 0;
        _handle5 = 0;
        _handle6 = 0;
        _handle7 = 0;
        _ctrl0 = null;
        _ctrl1 = null;
        _ctrl2 = null;
        _ctrl3 = null;
        _ctrl4 = null;
        _ctrl5 = null;
        _ctrl6 = null;
        _ctrl7 = null;
        _focusIndex = -1;
        _windowHandle = 0;
        _installed = 0;
    }

    internal static void RegisterTabStop(Control ctrl, long platformHandle) {
        if (ctrl == null || platformHandle == 0 || _tabCount >= 8) {
            return;
        }
        if (!ctrl.Focusable || !ctrl.IsTabStop || !ctrl.IsEnabled) {
            return;
        }
        if (_tabCount == 0) {
            _handle0 = platformHandle;
            _ctrl0 = ctrl;
        } else if (_tabCount == 1) {
            _handle1 = platformHandle;
            _ctrl1 = ctrl;
        } else if (_tabCount == 2) {
            _handle2 = platformHandle;
            _ctrl2 = ctrl;
        } else if (_tabCount == 3) {
            _handle3 = platformHandle;
            _ctrl3 = ctrl;
        } else if (_tabCount == 4) {
            _handle4 = platformHandle;
            _ctrl4 = ctrl;
        } else if (_tabCount == 5) {
            _handle5 = platformHandle;
            _ctrl5 = ctrl;
        } else if (_tabCount == 6) {
            _handle6 = platformHandle;
            _ctrl6 = ctrl;
        } else if (_tabCount == 7) {
            _handle7 = platformHandle;
            _ctrl7 = ctrl;
        }
        _tabCount = _tabCount + 1;
    }

    internal static void SetWindowHandle(long windowHandle) {
        _windowHandle = windowHandle;
    }

    internal static void Install() {
        if (_installed != 0) {
            return;
        }
        Action<int, int> handler = FocusManager.RouteKey;
        WindowHost.SetKeyboardHandler(handler);
        _installed = 1;
    }

    internal static void ActivateInitialFocus() {
        if (_tabCount == 0) {
            _focusIndex = -1;
            return;
        }
        FocusManager.SetFocusIndex(0);
    }

    static long HandleAt(int idx) {
        if (idx == 0) {
            return _handle0;
        }
        if (idx == 1) {
            return _handle1;
        }
        if (idx == 2) {
            return _handle2;
        }
        if (idx == 3) {
            return _handle3;
        }
        if (idx == 4) {
            return _handle4;
        }
        if (idx == 5) {
            return _handle5;
        }
        if (idx == 6) {
            return _handle6;
        }
        return _handle7;
    }

    static Control ControlAt(int idx) {
        if (idx == 0) {
            return _ctrl0;
        }
        if (idx == 1) {
            return _ctrl1;
        }
        if (idx == 2) {
            return _ctrl2;
        }
        if (idx == 3) {
            return _ctrl3;
        }
        if (idx == 4) {
            return _ctrl4;
        }
        if (idx == 5) {
            return _ctrl5;
        }
        if (idx == 6) {
            return _ctrl6;
        }
        return _ctrl7;
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
        FramePump.Invalidate();
    }

    static void SetFocusIndex(int idx) {
        int count = _tabCount;
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

    /// <summary>按平台句柄设置焦点（点击 TextBox / 外部同步 Tab 索引）。</summary>
    internal static bool FocusPlatformHandle(long platformHandle) {
        if (platformHandle == 0 || _tabCount <= 0) {
            return false;
        }
        int i = 0;
        while (i < _tabCount) {
            if (FocusManager.HandleAt(i) == platformHandle) {
                FocusManager.SetFocusIndex(i);
                return true;
            }
            i = i + 1;
        }
        return false;
    }

    internal static void RouteKey(int virtualKey, int shiftDown) {
        if (_tabCount == 0) {
            return;
        }
        // 键盘消费统一入口（InputElement.OnKeyDown 契约）：焦点元素优先消费
        // （TextBox 光标/编辑键经 native 通道处理，此处声明消费即止），
        // 未消费才进入 Tab/方向焦点导航——根治方向键双路由。
        if (_focusIndex >= 0 && _focusIndex < _tabCount) {
            Control focused = ControlAt(_focusIndex);
            if (focused is InputElement) {
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
                next = shiftDown != 0 ? _tabCount - 1 : 0;
            }
            FocusManager.SetFocusIndex(next);
            return;
        }
        // M5 方向导航（RFC 037 M5）：方向键沿 Tab 循环顺序移动焦点。
        // Left/Up 前移一位，Right/Down 后移一位（与 Tab 循环同一 registry）。
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
                next = directionDelta > 0 ? 0 : _tabCount - 1;
            }
            FocusManager.SetFocusIndex(next);
            return;
        }
        if (virtualKey == FocusManager.VirtualKeyReturn() ||
            virtualKey == FocusManager.VirtualKeySpace()) {
            if (_focusIndex < 0 || _focusIndex >= _tabCount) {
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
