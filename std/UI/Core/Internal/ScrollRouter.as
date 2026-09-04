// RFC 037 D10.5 · RFC 037 Internal: Win32 滚轮 + 竖滚动条 → Arc ScrollView Offset 路由。
// 多 ScrollView：按平台句柄映射（外层 + 嵌套各自独立；禁单槽覆盖）。

namespace Arc.UI.Internal;

using Arc.UI;
using Arc.UI.Components;
using Arc.UI.Components.Layout;
using Arc.UI.Layout;

/// <summary>平台滚轮/竖滚动条命中 ScrollView → 更新 Offset、重布局、同步平台镜像。</summary>
internal class ScrollRouter {
    static int _slotCount = 0;
    static long _handle0 = 0;
    static long _handle1 = 0;
    static long _handle2 = 0;
    static long _handle3 = 0;
    static long _handle4 = 0;
    static long _handle5 = 0;
    static long _handle6 = 0;
    static long _handle7 = 0;
    static ScrollView _scroll0 = null;
    static ScrollView _scroll1 = null;
    static ScrollView _scroll2 = null;
    static ScrollView _scroll3 = null;
    static ScrollView _scroll4 = null;
    static ScrollView _scroll5 = null;
    static ScrollView _scroll6 = null;
    static ScrollView _scroll7 = null;
    static Element _windowElement = null;
    static long _platformRoot = 0;
    static int _installed = 0;
    /// <summary>当前正在被拖拽的滚动条所属 ScrollView 平台句柄（0=无拖拽）。</summary>
    static long _draggingScrollHandle = 0;

    private ScrollRouter() {
    }

    /// <summary>每次 Show 前由 Window 调用。</summary>
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
        _scroll0 = null;
        _scroll1 = null;
        _scroll2 = null;
        _scroll3 = null;
        _scroll4 = null;
        _scroll5 = null;
        _scroll6 = null;
        _scroll7 = null;
        _platformRoot = 0;
        _installed = 0;
    }

    /// <summary>PlatformTreeSync 构建 ScrollView 平台节点后注册映射。</summary>
    internal static void RegisterScrollView(long platformHandle, ScrollView scrollView) {
        if (scrollView == null || platformHandle == 0 || _slotCount >= 8) {
            return;
        }
        if (_slotCount == 0) {
            _handle0 = platformHandle;
            _scroll0 = scrollView;
        } else if (_slotCount == 1) {
            _handle1 = platformHandle;
            _scroll1 = scrollView;
        } else if (_slotCount == 2) {
            _handle2 = platformHandle;
            _scroll2 = scrollView;
        } else if (_slotCount == 3) {
            _handle3 = platformHandle;
            _scroll3 = scrollView;
        } else if (_slotCount == 4) {
            _handle4 = platformHandle;
            _scroll4 = scrollView;
        } else if (_slotCount == 5) {
            _handle5 = platformHandle;
            _scroll5 = scrollView;
        } else if (_slotCount == 6) {
            _handle6 = platformHandle;
            _scroll6 = scrollView;
        } else if (_slotCount == 7) {
            _handle7 = platformHandle;
            _scroll7 = scrollView;
        }
        _slotCount = _slotCount + 1;
    }

    /// <summary>Window.Show 在 BuildFromArc 之后绑定根句柄。</summary>
    internal static void BindWindow(Window window, long platformRoot) {
        _windowElement = window;
        _platformRoot = platformRoot;
    }

    /// <summary>安装 C→Arc 滚轮 + 竖滚动条回调（PrepareForShow）。</summary>
    internal static void Install() {
        Action<long, int, int> wheelHandler = ScrollRouter.RouteWheel;
        WindowHost.SetScrollWheelHandler(wheelHandler);
        Action<long, int, double> barHandler = ScrollRouter.RouteScrollBar;
        WindowHost.SetScrollBarHandler(barHandler);
        _installed = 1;
    }

    static ScrollView Lookup(long platformHandle) {
        if (_installed == 0 || platformHandle == 0) {
            return null;
        }
        if (_slotCount > 0 && _handle0 == platformHandle) {
            return _scroll0;
        }
        if (_slotCount > 1 && _handle1 == platformHandle) {
            return _scroll1;
        }
        if (_slotCount > 2 && _handle2 == platformHandle) {
            return _scroll2;
        }
        if (_slotCount > 3 && _handle3 == platformHandle) {
            return _scroll3;
        }
        if (_slotCount > 4 && _handle4 == platformHandle) {
            return _scroll4;
        }
        if (_slotCount > 5 && _handle5 == platformHandle) {
            return _scroll5;
        }
        if (_slotCount > 6 && _handle6 == platformHandle) {
            return _scroll6;
        }
        if (_slotCount > 7 && _handle7 == platformHandle) {
            return _scroll7;
        }
        return null;
    }

    static int _relayoutInProgress = 0; // 重入保护：布局循环防御

    static void RelayoutAndDirty() {
        if (_relayoutInProgress != 0) {
            return; // 重入：布局中触发布局 → 跳过，防无限循环
        }
        _relayoutInProgress = 1;
        if (_windowElement == null || _platformRoot == 0) {
            _relayoutInProgress = 0;
            return;
        }
        Window win = (Window)_windowElement;
        LayoutManager.Update(win);
        PlatformTreeSync.SyncLayoutFromArc(_windowElement, _platformRoot);
        FramePump.Invalidate();
        WindowHost.InvalidateActiveWindow();
        _relayoutInProgress = 0;
    }

    /// <summary>C 回调：platform handle + 滚轮 delta（Win32 GET_WHEEL_DELTA_WPARAM）。</summary>
    internal static void RouteWheel(long platformHandle, int deltaX, int deltaY) {
        Console.WriteLine("[SCROLL-DIAG] RouteWheel handle=" + (int)platformHandle + " dx=" + deltaX + " dy=" + deltaY);
        ScrollView scroll = ScrollRouter.Lookup(platformHandle);
        if (scroll == null) {
            return;
        }
        double step = (double)deltaY / 120.0 * 48.0;
        scroll.ApplyWheelDelta(0.0, 0.0 - step);
        ScrollRouter.RelayoutAndDirty();
    }

    /// <summary>C 回调：竖滚动条拖拽/轨道点击 → 更新 VerticalOffset。</summary>
    internal static void RouteScrollBar(long platformHandle, int action, double value) {
        ScrollView scroll = ScrollRouter.Lookup(platformHandle);
        if (scroll == null) {
            return;
        }
        if (action == 1) {
            scroll.SetVerticalOffsetClamped(value);
        } else if (action == 2) {
            double page = scroll.ViewportHeight * 0.9;
            scroll.SetVerticalOffsetClamped(scroll.VerticalOffset - page);
        } else if (action == 3) {
            double page = scroll.ViewportHeight * 0.9;
            scroll.SetVerticalOffsetClamped(scroll.VerticalOffset + page);
        } else if (action == 4) {
            _draggingScrollHandle = platformHandle;
        } else if (action == 5) {
            _draggingScrollHandle = 0;
        }
        ScrollRouter.RelayoutAndDirty();
    }

    /// <summary>查询指定 ScrollView 平台句柄是否正在被拖拽（供渲染层读取 pressed 状态）。</summary>
    internal static bool IsDragging(long platformHandle) {
        return _draggingScrollHandle == platformHandle && platformHandle != 0;
    }
}
