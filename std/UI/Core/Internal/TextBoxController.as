// RFC 037 §8 修订（text-editing.md §4）：TextBoxController——键命令/指针/IME
// 事件 → TextBoxModel 内核操作的唯一映射层。
//
// 编辑语义唯一真相在内核（D6 消除）：本类只做事件翻译——KeyboardRouter
// OnKey/OnText 与 ImeBridge 组字通道翻译为内核操作，调用后统一经
// TextBox.SyncFromModel 同步 DP/事件/镜像。组字期路由判断（§3.4：Backspace
// 交 IME）在本层——内核不感知平台。
//
// IN-R2：HandleKey(vk, mods) 承接编辑键（含 Ctrl+A/Z/Y/C/V/X、方向/Home/End/
// Delete）；平台层禁做修饰键分支与 IsReadOnly 门控。
// 剪贴板：TextBox Copy/Cut/Paste；PasswordBox 仅 Paste（禁明文出剪贴板）。

namespace Arc.UI.Internal;

using Arc.UI.Components;
using Arc.UI.Editing;
using Arc.UI.Layout;

/// <summary>
/// TextBox 事件控制器：平台事件 → 内核操作映射（编辑语义不落地本层）。
/// </summary>
internal static class TextBoxController {
    static int VirtualKeyA() {
        return 65;
    }

    static int VirtualKeyC() {
        return 67;
    }

    static int VirtualKeyV() {
        return 86;
    }

    static int VirtualKeyX() {
        return 88;
    }

    static int VirtualKeyZ() {
        return 90;
    }

    static int VirtualKeyY() {
        return 89;
    }

    /// <summary>
    /// 键命令映射（KeyboardRouter）：消费则 true（阻止焦点导航）；
    /// Tab/Enter/Space 返回 false 交 FocusManager。
    /// </summary>
    public static bool HandleKey(TextBox box, int virtualKey, int mods) {
        if (box == null) {
            return false;
        }
        if (virtualKey == FocusManager.VirtualKeyTab()
            || virtualKey == FocusManager.VirtualKeyReturn()) {
            return false;
        }
        // Space：TextBox 插入空格并消费（禁误入 Button 式 Activate）；
        // 非 TextBox 焦点由 RouteKey Activate 处理。
        if (virtualKey == FocusManager.VirtualKeySpace()) {
            TextBoxController.HandleAscii(box, " ");
            return true;
        }
        bool shift = (mods & KeyboardRouter.ModShift()) != 0;
        bool ctrl = (mods & KeyboardRouter.ModCtrl()) != 0;
        TextBoxModel model = box.Model();
        if (model.Composition != "") {
            // 组字期编辑键交 IME（平台已 gate；此处仍声明消费防导航抢键）。
            if (virtualKey == FocusManager.VirtualKeyLeft()
                || virtualKey == FocusManager.VirtualKeyRight()
                || virtualKey == FocusManager.VirtualKeyUp()
                || virtualKey == FocusManager.VirtualKeyDown()
                || virtualKey == FocusManager.VirtualKeyHome()
                || virtualKey == FocusManager.VirtualKeyEnd()
                || virtualKey == FocusManager.VirtualKeyBackspace()
                || virtualKey == FocusManager.VirtualKeyDelete()) {
                return true;
            }
        }
        if (ctrl && virtualKey == VirtualKeyA()) {
            TextBoxController.HandleSelectAll(box);
            return true;
        }
        if (ctrl && virtualKey == VirtualKeyC()) {
            TextBoxController.HandleCopy(box);
            return true;
        }
        if (ctrl && virtualKey == VirtualKeyX()) {
            TextBoxController.HandleCut(box);
            return true;
        }
        if (ctrl && virtualKey == VirtualKeyV()) {
            TextBoxController.HandlePaste(box);
            return true;
        }
        if (ctrl && virtualKey == VirtualKeyZ()) {
            if (model.Undo()) {
                box.SyncFromModel(false);
            }
            return true;
        }
        if (ctrl && virtualKey == VirtualKeyY()) {
            if (model.Redo()) {
                box.SyncFromModel(false);
            }
            return true;
        }
        if (virtualKey == FocusManager.VirtualKeyLeft()) {
            MoveGranularity gran = MoveGranularity.Char;
            if (ctrl) {
                gran = MoveGranularity.Word;
            }
            model.MoveCaret(MoveDirection.Backward, gran, shift);
            box.SyncFromModel(false);
            return true;
        }
        if (virtualKey == FocusManager.VirtualKeyRight()) {
            MoveGranularity gran = MoveGranularity.Char;
            if (ctrl) {
                gran = MoveGranularity.Word;
            }
            model.MoveCaret(MoveDirection.Forward, gran, shift);
            box.SyncFromModel(false);
            return true;
        }
        if (virtualKey == FocusManager.VirtualKeyHome()) {
            TextBoxController.HandleHome(box, shift);
            return true;
        }
        if (virtualKey == FocusManager.VirtualKeyEnd()) {
            TextBoxController.HandleEnd(box, shift);
            return true;
        }
        if (virtualKey == FocusManager.VirtualKeyBackspace()) {
            TextBoxController.HandleBackspace(box);
            return true;
        }
        if (virtualKey == FocusManager.VirtualKeyDelete()) {
            TextBoxController.HandleDelete(box);
            return true;
        }
        // 单行 TextBox：上下键消费但不移动（禁误入焦点导航）。
        if (virtualKey == FocusManager.VirtualKeyUp()
            || virtualKey == FocusManager.VirtualKeyDown()) {
            return true;
        }
        return false;
    }

    /// <summary>可打印字符直输（WM_CHAR → rt_ui_dispatch_text）。</summary>
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

    /// <summary>Ctrl+C：有选区且允许导出时写剪贴板；PasswordBox 空操作。</summary>
    public static void HandleCopy(TextBox box) {
        if (box == null || !box.AllowsClipboardCopy()) {
            return;
        }
        TextBoxModel model = box.Model();
        if (model.SelectionLength <= 0) {
            return;
        }
        WindowHost.ClipboardSetText(model.SelectedText);
    }

    /// <summary>Ctrl+X：Copy + 删选区；PasswordBox 禁导出故整命令空操作。</summary>
    public static void HandleCut(TextBox box) {
        if (box == null || !box.AllowsClipboardCopy()) {
            return;
        }
        TextBoxModel model = box.Model();
        if (model.SelectionLength <= 0 || model.IsReadOnly) {
            return;
        }
        WindowHost.ClipboardSetText(model.SelectedText);
        model.DeleteForward();
        box.SyncFromModel(false);
    }

    /// <summary>Ctrl+V：剪贴板文本插入（单行剥离 CR/LF）；PasswordBox 允许。</summary>
    public static void HandlePaste(TextBox box) {
        if (box == null) {
            return;
        }
        TextBoxModel model = box.Model();
        if (model.IsReadOnly) {
            return;
        }
        string raw = WindowHost.ClipboardGetText();
        string chunk = TextBoxController.SanitizePaste(raw);
        if (chunk == null || chunk == "") {
            return;
        }
        if (model.Composition != "") {
            model.CancelComposition();
        }
        model.Insert(chunk);
        box.SyncFromModel(false);
    }

    /// <summary>单行粘贴：去掉 CR/LF，避免多行内容撑破单行契约。</summary>
    static string SanitizePaste(string raw) {
        if (raw == null || raw == "") {
            return "";
        }
        string result = "";
        int i = 0;
        int n = raw.Length;
        while (i < n) {
            string ch = raw.Substring(i, 1);
            if (ch != "\r" && ch != "\n") {
                result = result + ch;
            }
            i = i + 1;
        }
        return result;
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
        cache.Ensure(box.GeometryText(), model.Version, fontSize, box.FontFamily, box.FontWeight);
        int idx = cache.NearestIndexTo(localDipX - InputMetrics.PenOriginX);
        model.SetCaret(idx);
        box.SyncFromModel(false);
    }

    /// <summary>
    /// 双击词选：命中 DIP → 码点索引 → <see cref="TextBoxModel.SelectWordAt"/>。
    /// </summary>
    public static void HandleDoubleClick(TextBox box, double localDipX) {
        if (box == null) {
            return;
        }
        TextBoxModel model = box.Model();
        double fontSize = box.FontSize;
        if (fontSize <= 0.0) {
            fontSize = InputMetrics.FontSizeFallback;
        }
        PrefixWidthCache cache = box.PrefixCache();
        cache.Ensure(box.GeometryText(), model.Version, fontSize, box.FontFamily, box.FontWeight);
        int idx = cache.NearestIndexTo(localDipX - InputMetrics.PenOriginX);
        model.SelectWordAt(idx);
        box.SyncFromModel(false);
    }

    /// <summary>
    /// 拖拽扩展选区：保持点击时的 Anchor，活动端跟局部 DIP 横坐标。
    /// </summary>
    public static void HandleDrag(TextBox box, double localDipX) {
        if (box == null) {
            return;
        }
        TextBoxModel model = box.Model();
        double fontSize = box.FontSize;
        if (fontSize <= 0.0) {
            fontSize = InputMetrics.FontSizeFallback;
        }
        PrefixWidthCache cache = box.PrefixCache();
        cache.Ensure(box.GeometryText(), model.Version, fontSize, box.FontFamily, box.FontWeight);
        int idx = cache.NearestIndexTo(localDipX - InputMetrics.PenOriginX);
        model.SetSelection(model.Anchor, idx);
        box.SyncFromModel(false);
    }
}
