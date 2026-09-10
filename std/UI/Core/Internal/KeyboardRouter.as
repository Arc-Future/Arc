// RFC 037 §8 IN-R2：单一键盘通道。
//
// 平台只做机械转换（WM_KEYDOWN → rt_ui_dispatch_key / WM_CHAR →
// rt_ui_dispatch_text）；本类按焦点类型分发：TextBox → TextBoxController
// 编辑命令，其余 → FocusManager Tab/方向导航与 Activate。

namespace Arc.UI.Internal;

using Arc.UI;
using Arc.UI.Components;
using Arc.UI.Input;

/// <summary>键盘/文本单一入口：按焦点类型分发编辑或导航。</summary>
internal class KeyboardRouter {
    static int _installed;

    private KeyboardRouter() {
    }

    /// <summary>mods 位：bit0 = Shift，bit1 = Ctrl（RFC 037 §8）。</summary>
    internal static int ModShift() {
        return 1;
    }

    internal static int ModCtrl() {
        return 2;
    }

    /// <summary>Application / FocusManager 启动时安装 key+text 双通道。</summary>
    internal static void Install() {
        if (_installed != 0) {
            return;
        }
        Action<int, int> keyHandler = KeyboardRouter.OnKey;
        Action<long> textHandler = KeyboardRouter.OnText;
        WindowHost.SetKeyHandler(keyHandler);
        WindowHost.SetTextHandler(textHandler);
        _installed = 1;
    }

    /// <summary>平台 WM_KEYDOWN：vk + mods → 弹层 Esc / 编辑 / 焦点导航。</summary>
    internal static void OnKey(int virtualKey, int mods) {
        // Esc 优先：轻关闭顶层 Popup（严格 LIFO，不穿透非轻关闭顶层）
        // → MessageBox 模态自管 → 有活跃弹层则勿退窗 → 否则关主窗。
        if (virtualKey == FocusManager.VirtualKeyEscape()) {
            if (Popup.TryDismissTopOnEscape()) {
                return;
            }
            if (MessageBox.TryHandleEscape()) {
                return;
            }
            if (Popup.HasActivePopup()) {
                return;
            }
            if (Application.Current != null && Application.Current.MainWindow != null) {
                Application.Current.MainWindow.Close();
            }
            return;
        }
        int shiftDown = (mods & KeyboardRouter.ModShift()) != 0 ? 1 : 0;
        TextBox box = ImeBridge.FocusedInput();
        if (box != null) {
            if (TextBoxController.HandleKey(box, virtualKey, mods)) {
                return;
            }
        }
        FocusManager.RouteKey(virtualKey, shiftDown);
    }

    /// <summary>平台 WM_CHAR：UTF-8 指针 → TextBox 插入（只读由内核裁决）。</summary>
    internal static void OnText(long utf8Ptr) {
        if (utf8Ptr == 0) {
            return;
        }
        TextBox box = ImeBridge.FocusedInput();
        if (box == null) {
            return;
        }
        string chunk = WindowHost.NativeCStringFromPtr(utf8Ptr);
        TextBoxController.HandleAscii(box, chunk);
    }
}
