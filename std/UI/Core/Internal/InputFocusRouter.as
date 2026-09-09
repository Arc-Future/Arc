// RFC 037 §8：平台 TextBox 命中 → Arc ImeBridge 焦点路由。
//
// 动态容量（List）：与 FocusManager 同构，禁 8 槽静默丢弃。
// SoftCapacity=64 起溢出告警，仍继续登记。

namespace Arc.UI.Internal;

using Arc.Collections;
using Arc.Diagnostics;
using Arc.UI.Components;
using Arc.UI.Input;

internal class InputFocusRouter {
    const int SoftCapacity = 64;

    static List<long> _handles = new List<long>();
    static List<TextBox> _inputs = new List<TextBox>();
    static int _registered = 0;

    private InputFocusRouter() {
    }

    internal static void Reset() {
        _handles.Clear();
        _inputs.Clear();
        _registered = 0;
    }

    internal static void RegisterInput(long platformHandle, TextBox input) {
        if (input == null || platformHandle == 0) {
            return;
        }
        int count = _handles.Count;
        if (count >= SoftCapacity) {
            Console.WriteLine("[InputFocusRouter] input registry soft capacity exceeded ("
                + SoftCapacity + "); registering anyway (count=" + count + ")");
        }
        _handles.Add(platformHandle);
        _inputs.Add(input);
        _registered = 1;
    }

    internal static void Install() {
        Action<long> handler = InputFocusRouter.RouteFocus;
        WindowHost.SetInputFocusHandler(handler);
    }

    internal static void RouteFocus(long platformHandle) {
        // 优先经 FocusManager：同步 Tab 索引 + IsFocused 镜像 + IME。
        if (FocusManager.FocusPlatformHandle(platformHandle)) {
            FramePump.Invalidate();
            return;
        }
        TextBox input = Lookup(platformHandle);
        if (input != null) {
            ImeBridge.SetFocused(input);
            FramePump.Invalidate();
            _registered = 1;
            return;
        }
        Console.WriteLine("[InputFocusRouter] focus miss handle=" + platformHandle);
    }

    static TextBox Lookup(long platformHandle) {
        if (platformHandle == 0 || _registered == 0) {
            return null;
        }
        int count = _handles.Count;
        int i = 0;
        while (i < count) {
            if (_handles[i] == platformHandle) {
                return _inputs[i];
            }
            i = i + 1;
        }
        return null;
    }
}