// RFC 037 M-caret1 · RFC 037 Internal: platform TextBox hit -> Arc ImeBridge focus routing.
//
// Draft: fixed-slot registry (List/long[] monomorphization not ready for Show).
//
// 【已知根因 · RFC 037 §8 修订（M-focus2）】8 槽 slot 满后静默丢弃注册——与 FocusManager
// 双 miss 即「点击无焦点」死链（ArmlDemo NameInput 实锤）。本类待 IN-R3 并入
// FocusManager 单一焦点权威（点击改 rt_ui_dispatch_input_activated 单通道），见 docs/plan.md「UI 输入栈重构」。

namespace Arc.UI.Internal;

using Arc.UI.Components;
using Arc.UI.Input;

internal class InputFocusRouter {
    static int _slotCount = 0;
    static long _handle0 = 0;
    static long _handle1 = 0;
    static long _handle2 = 0;
    static long _handle3 = 0;
    static long _handle4 = 0;
    static long _handle5 = 0;
    static long _handle6 = 0;
    static long _handle7 = 0;
    static TextBox _input0 = null;
    static TextBox _input1 = null;
    static TextBox _input2 = null;
    static TextBox _input3 = null;
    static TextBox _input4 = null;
    static TextBox _input5 = null;
    static TextBox _input6 = null;
    static TextBox _input7 = null;
    static int _registered = 0;

    private InputFocusRouter() {
    }

    internal static void Reset() {
        _slotCount = 0;
        _handle0 = 0;
        _handle1 = 0;
        _handle2 = 0;
        _handle3 = 0;
        _handle4 = 0;
        _handle5 = 0;
        _handle6 = 0;
        _handle7 = 0;
        _input0 = null;
        _input1 = null;
        _input2 = null;
        _input3 = null;
        _input4 = null;
        _input5 = null;
        _input6 = null;
        _input7 = null;
        _registered = 0;
    }

    internal static void RegisterInput(long platformHandle, TextBox input) {
        if (input == null || platformHandle == 0 || _slotCount >= 8) {
            return;
        }
        if (_slotCount == 0) {
            _handle0 = platformHandle;
            _input0 = input;
        } else if (_slotCount == 1) {
            _handle1 = platformHandle;
            _input1 = input;
        } else if (_slotCount == 2) {
            _handle2 = platformHandle;
            _input2 = input;
        } else if (_slotCount == 3) {
            _handle3 = platformHandle;
            _input3 = input;
        } else if (_slotCount == 4) {
            _handle4 = platformHandle;
            _input4 = input;
        } else if (_slotCount == 5) {
            _handle5 = platformHandle;
            _input5 = input;
        } else if (_slotCount == 6) {
            _handle6 = platformHandle;
            _input6 = input;
        } else if (_slotCount == 7) {
            _handle7 = platformHandle;
            _input7 = input;
        }
        _slotCount = _slotCount + 1;
    }

    internal static void Install() {
        Action<long> handler = InputFocusRouter.RouteFocus;
        WindowHost.SetInputFocusHandler(handler);
    }

    internal static void RouteFocus(long platformHandle) {
        // 优先走 FocusManager：同步 Tab 索引 + IsFocused 镜像 + IME。
        if (FocusManager.FocusPlatformHandle(platformHandle)) {
            FramePump.Invalidate();
            return;
        }
        TextBox input = LookupInput(platformHandle);
        if (input != null) {
            ImeBridge.SetFocused(input);
            FramePump.Invalidate();
            _registered = 1;
        }
    }

    static TextBox LookupInput(long platformHandle) {
        if (platformHandle == 0) {
            return null;
        }
        if (_slotCount > 0 && _handle0 == platformHandle) {
            return _input0;
        }
        if (_slotCount > 1 && _handle1 == platformHandle) {
            return _input1;
        }
        if (_slotCount > 2 && _handle2 == platformHandle) {
            return _input2;
        }
        if (_slotCount > 3 && _handle3 == platformHandle) {
            return _input3;
        }
        if (_slotCount > 4 && _handle4 == platformHandle) {
            return _input4;
        }
        if (_slotCount > 5 && _handle5 == platformHandle) {
            return _input5;
        }
        if (_slotCount > 6 && _handle6 == platformHandle) {
            return _input6;
        }
        if (_slotCount > 7 && _handle7 == platformHandle) {
            return _input7;
        }
        return null;
    }
}
