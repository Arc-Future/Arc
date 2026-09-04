// RFC 037 §8 修订（text-editing.md §4）：TextBoxController——键命令/指针/IME
// 事件 → TextBoxModel 内核操作的唯一映射层。
//
// 编辑语义唯一真相在内核（D6 消除）：本类只做事件翻译——ImeBridge 的平台
// Kind（C 层机械转换，不做编辑判断）翻译为内核操作调用，调用后统一经
// TextBox.SyncFromModel 同步 DP/事件/镜像。组字期路由判断（§3.4：Backspace
// 交 IME）在本层——内核不感知平台。
//
// 归属：std/UI/Internal/（internal 机制；text-editing.md §4 原文
// std/UI/Input/ 随目录归并 Internal/ 同步）。
//
// 将来 IN-R2（KeyboardRouter 单一键盘通道，rt_ui_dispatch_key/text）落地后，
// 本类增加 HandleKey(vk, ctrl, shift) 完整映射（Ctrl+A/Z/Y/C/V、Ctrl+方向
// 词粒度），C 层编辑键分支随之删除。

namespace Arc.UI.Internal;

using Arc.UI.Components;
using Arc.UI.Editing;
using Arc.UI.Layout;

/// <summary>
/// TextBox 事件控制器：平台事件 → 内核操作映射（编辑语义不落地本层）。
/// </summary>
internal static class TextBoxController {
    /// <summary>ASCII 可打印字符直输（WM_CHAR → RT_UI_IME_ASCII_CHAR）。</summary>
    public static void HandleAscii(TextBox box, string ch) {
        if (box == null || ch == null || ch == "") {
            return;
        }
        TextBoxModel model = box.Model();
        // 组字期非 IME 通道字符：先取消组字再插入（桌面行为近似——组字串
        // 归 IME 管理，直输通道不与之混排）。
        if (model.Composition != "") {
            model.CancelComposition();
        }
        model.Insert(ch);
        box.SyncFromModel(false);
    }

    /// <summary>Backspace：组字期交 IME（C 层更新 composition，内核不动）。</summary>
    public static void HandleBackspace(TextBox box) {
        if (box == null) {
            return;
        }
        TextBoxModel model = box.Model();
        if (model.Composition != "") {
            return;
        }
        model.DeleteBackward();
        box.SyncFromModel(false);
    }

    /// <summary>Delete 前向删除。</summary>
    public static void HandleDelete(TextBox box) {
        if (box == null) {
            return;
        }
        box.Model().DeleteForward();
        box.SyncFromModel(false);
    }

    /// <summary>方向键移动光标（extend 对应 Shift 扩选）。</summary>
    public static void HandleCaretChar(TextBox box, bool backward, bool extend) {
        if (box == null) {
            return;
        }
        MoveDirection direction = MoveDirection.Forward;
        if (backward) {
            direction = MoveDirection.Backward;
        }
        box.Model().MoveCaret(direction, MoveGranularity.Char, extend);
        box.SyncFromModel(false);
    }

    /// <summary>Home（extend 对应 Shift 到行首扩选）。</summary>
    public static void HandleHome(TextBox box, bool extend) {
        if (box == null) {
            return;
        }
        box.Model().MoveCaret(MoveDirection.Backward, MoveGranularity.Home, extend);
        box.SyncFromModel(false);
    }

    /// <summary>End（extend 对应 Shift 到行尾扩选）。</summary>
    public static void HandleEnd(TextBox box, bool extend) {
        if (box == null) {
            return;
        }
        box.Model().MoveCaret(MoveDirection.Forward, MoveGranularity.End, extend);
        box.SyncFromModel(false);
    }

    /// <summary>Ctrl+A 全选。</summary>
    public static void HandleSelectAll(TextBox box) {
        if (box == null) {
            return;
        }
        box.Model().SelectAll();
        box.SyncFromModel(false);
    }

    /// <summary>IME 上屏（commit 是独立撤销单元）。</summary>
    public static void HandleCommit(TextBox box, string chunk) {
        if (box == null) {
            return;
        }
        if (chunk != null && chunk != "") {
            box.Model().CommitComposition(chunk);
        } else {
            box.Model().CancelComposition();
        }
        box.SyncFromModel(false);
    }

    /// <summary>IME 组字预览更新（不进 text，不影响撤销栈）。</summary>
    public static void HandleComposition(TextBox box, string text) {
        if (box == null) {
            return;
        }
        box.Model().SetComposition(text);
        box.SyncFromModel(false);
    }

    /// <summary>
    /// 点击定位光标（局部 DIP 横坐标）：前缀宽度缓存最近字符边界原则。
    /// 几何与渲染端同源（InputMetrics.PenOriginX 单点）。
    /// </summary>
    public static void HandleClick(TextBox box, double localDipX) {
        if (box == null) {
            return;
        }
        TextBoxModel model = box.Model();
        double fontSize = box.FontSize;
        if (fontSize <= 0.0) {
            fontSize = InputMetrics.FontSizeFallback;
        }
        PrefixWidthCache cache = box.PrefixCache();
        cache.Ensure(model.Text, model.Version, fontSize, box.FontFamily, box.FontWeight);
        int idx = cache.NearestIndexTo(localDipX - InputMetrics.PenOriginX);
        model.SetCaret(idx);
        box.SyncFromModel(false);
    }
}
