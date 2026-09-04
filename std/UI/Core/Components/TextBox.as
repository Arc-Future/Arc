// RFC 037 D2.1 + RFC 037 §8 修订（text-editing.md §2–§4）：TextBox 单行文本输入框。
//
// 分层职责（text-editing.md §4）：
//   - 本类是 DP 壳：Text/Placeholder/IsReadOnly/MaxLength 属性面 +
//     TextChanged/SelectionChanged 事件 + Measure + 平台镜像同步；
//   - 编辑语义唯一真相在 TextBoxModel（Arc.UI.Editing，internal，
//     headless 可测）；键命令/指针/IME 事件映射在 TextBoxController
//     （Arc.UI.Internal，internal）——本类不再散布编辑状态机。
//
// 命名（§2 硬改名）：Input → TextBox（XAML 系统一惯用法；Block=只读 /
// Box=可编辑对仗）。mirror 属性名字符串（"Text"/"CaretIndex" 等）是属性面，
// 不受元素改名影响；rt_* ABI 无元素名字符串。
//
// IME commit 经 ImeBridge → TextBoxController 写入内核；平台镜像同步
// Text/CompositionText/CaretIndex/Selection*/IsFocused。

namespace Arc.UI.Components;

using Arc.UI.Editing;
using Arc.UI.Input;
using Arc.UI.Internal;
using Arc.UI.Layout;

/// <summary>单行文本输入控件（编辑内核 TextBoxModel 的 DP 壳）。</summary>
public class TextBox : InputElement {
    public static DependencyProperty<string> TextProperty =
        RegisterProperty<string>(nameof(Text), typeof(TextBox), "");

    public static DependencyProperty<string> PlaceholderProperty =
        RegisterProperty<string>(nameof(Placeholder), typeof(TextBox), "");

    public static DependencyProperty<bool> IsReadOnlyProperty =
        RegisterProperty<bool>(nameof(IsReadOnly), typeof(TextBox), false);

    public static DependencyProperty<int> MaxLengthProperty =
        RegisterProperty<int>(nameof(MaxLength), typeof(TextBox), 0);

    /// <summary>插入符位置（字符索引；与选区活动端同值）。</summary>
    public static DependencyProperty<int> CaretIndexProperty =
        RegisterProperty<int>(nameof(CaretIndex), typeof(TextBox), 0);

    /// <summary>组字预览（IME overlay；不参与绑定）。</summary>
    public static DependencyProperty<string> CompositionTextProperty =
        RegisterProperty<string>(nameof(CompositionText), typeof(TextBox), "");

    TextBoxModel _model;
    PrefixWidthCache _prefixCache;
    string _syncedText = "";
    int _syncedStart;
    int _syncedLength;

    public TextBox() {
        this.Type = typeof(TextBox);
        _model = new TextBoxModel();
        _prefixCache = new PrefixWidthCache();
        this.TextChanged = new Signal<string>("");
        this.SelectionChanged = new Signal<int>(0);
    }

    // ===== 控件事件通道（RFC 037 §5.3 · Signal 单引擎）=====
    //
    // TextChanged：文本变更通知（载荷=新文本值）。捕获程序化赋值与内核编辑
    // 全部路径；ctor 不赋 Text，首次显式变更才触发。Signal.Set 无相等性短路
    // （与 string 属性无条件通知语义一致）。On* 便捷订阅内部弃 token（控件
    // 生命周期内常驻订阅、随元素销毁确定退订）。
    //
    // SelectionChanged：选区/光标变更通知（载荷=SelectionStart；长度经
    // SelectionLength 读取），选区或光标实际变化时触发。

    /// <summary>文本变更信号——载荷为新文本值。</summary>
    public Signal<string> TextChanged;

    /// <summary>选区/光标变更信号——载荷为 SelectionStart。</summary>
    public Signal<int> SelectionChanged;

    /// <summary>订阅文本变更——TextChanged.Subscribe 便捷封装。</summary>
    /// <param name="handler">变更回调（接收新文本值）。</param>
    public void OnTextChanged(Action<string> handler) {
        if (TextChanged != null && handler != null) {
            TextChanged.Subscribe(handler);
        }
    }

    /// <summary>订阅选区/光标变更——SelectionChanged.Subscribe 便捷封装。</summary>
    /// <param name="handler">变更回调（接收 SelectionStart）。</summary>
    public void OnSelectionChanged(Action<int> handler) {
        if (SelectionChanged != null && handler != null) {
            SelectionChanged.Subscribe(handler);
        }
    }

    // ===== 公共属性面（wrapper 委托 DP 槽；编辑策略同步内核）=====

    /// <summary>已提交文本（程序化赋值走内核 SetText：独立撤销快照）。</summary>
    public string Text {
        get { return this.GetValue<string>(TextProperty); }
        set {
            _model.SetText(value);
            this.SyncFromModel(true);
        }
    }

    public string Placeholder {
        get { return this.GetValue<string>(PlaceholderProperty); }
        set { this.SetValue<string>(PlaceholderProperty, value); }
    }

    public bool IsReadOnly {
        get { return this.GetValue<bool>(IsReadOnlyProperty); }
        set {
            this.SetValue<bool>(IsReadOnlyProperty, value);
            _model.IsReadOnly = value;
        }
    }

    public int MaxLength {
        get { return this.GetValue<int>(MaxLengthProperty); }
        set {
            this.SetValue<int>(MaxLengthProperty, value);
            _model.MaxLength = value;
        }
    }

    /// <summary>插入符位置（程序化设置：选区收敛为点）。</summary>
    public int CaretIndex {
        get { return this.GetValue<int>(CaretIndexProperty); }
        set {
            _model.SetCaret(value);
            this.SyncFromModel(false);
        }
    }

    /// <summary>组字预览（镜像面；源在内核，经 Controller 驱动）。</summary>
    public string CompositionText {
        get { return this.GetValue<string>(CompositionTextProperty); }
        set {
            _model.SetComposition(value);
            this.SyncFromModel(false);
        }
    }

    /// <summary>选区起点（派生只读：归一化区间下端）。</summary>
    public int SelectionStart {
        get { return _model.SelectionStart; }
    }

    /// <summary>选区长度（派生只读：0 = 无选区）。</summary>
    public int SelectionLength {
        get { return _model.SelectionLength; }
    }

    // ===== 编程式编辑入口（委托编辑内核；键盘/IME/程序化共道，单一惯用法）=====

    /// <summary>光标处插入文本（连续插入合并撤销快照）。</summary>
    public void Insert(string text) {
        if (text == null) {
            return;
        }
        _model.Insert(text);
        this.SyncFromModel(false);
    }

    /// <summary>提交 IME 组字片段（独立撤销快照并清除组字态）。</summary>
    public void CommitComposition(string chunk) {
        _model.CommitComposition(chunk);
        this.SyncFromModel(false);
    }

    /// <summary>退格删除（有选区删选区，否则删光标前一字符）。</summary>
    public void DeleteBackward() {
        _model.DeleteBackward();
        this.SyncFromModel(false);
    }

    /// <summary>撤销上一步编辑（无可撤销项返回 false）。</summary>
    public bool Undo() {
        bool ok = _model.Undo();
        if (ok) {
            this.SyncFromModel(false);
        }
        return ok;
    }

    /// <summary>重做（无可重做项返回 false）。</summary>
    public bool Redo() {
        bool ok = _model.Redo();
        if (ok) {
            this.SyncFromModel(false);
        }
        return ok;
    }

    /// <summary>全选。</summary>
    public void SelectAll() {
        _model.SelectAll();
        this.SyncFromModel(false);
    }

    internal bool HasSelection() {
        return _model.SelectionLength > 0;
    }

    /// <summary>编辑内核（TextBoxController 转发目标）。</summary>
    internal TextBoxModel Model() {
        return _model;
    }

    /// <summary>前缀宽度缓存（点击定位几何；按内核 Version 失效）。</summary>
    internal PrefixWidthCache PrefixCache() {
        return _prefixCache;
    }

    /// <summary>
    /// 内核状态 → DP 槽/事件/镜像的单向同步（Model → TextBox DP →
    /// mirror → 渲染）。forceNotify 强制触发 TextChanged（程序化 Text
    /// 赋值保持无条件通知语义）；内核编辑路径按实际变化触发。
    /// </summary>
    internal void SyncFromModel(bool forceNotify) {
        string text = _model.Text;
        int start = _model.SelectionStart;
        int length = _model.SelectionLength;
        this.SetValue<string>(TextProperty, text);
        this.SetValue<string>(CompositionTextProperty, _model.Composition);
        this.SetValue<int>(CaretIndexProperty, _model.Caret);
        bool textChanged = forceNotify || text != _syncedText;
        bool selectionChanged = start != _syncedStart || length != _syncedLength;
        _syncedText = text;
        _syncedStart = start;
        _syncedLength = length;
        if (textChanged && TextChanged != null) {
            TextChanged.Set(text);
        }
        if (selectionChanged && SelectionChanged != null) {
            SelectionChanged.Set(start);
        }
        FramePump.ResetCaretBlink();
        this.SyncMirrorText();
    }

    // ===== 平台镜像与焦点（属性面字符串不受元素改名影响）=====

    /// <summary>镜像登记（override）：扩展 IME 焦点候选与输入路由注册（幂等）。</summary>
    public override void BindPlatformMirror(long handle) {
        if (_mirrorHandle == handle) {
            return;
        }
        base.BindPlatformMirror(handle);
        ImeBridge.RegisterInput(this);
        InputFocusRouter.RegisterInput(handle, this);
        this.SyncMirrorText();
    }

    internal long MirrorHandle() {
        return _mirrorHandle;
    }

    /// <summary>焦点视觉同步（override）：基类镜像 IsFocused + caret 闪烁复位 + 文本镜像。</summary>
    protected override void OnFocusedChanged(bool focused) {
        base.OnFocusedChanged(focused);
        if (focused) {
            FramePump.ResetCaretBlink();
        }
        this.SyncMirrorText();
    }

    /// <summary>获得焦点钩子（override）：接管 IME 焦点（候选窗跟随 caret）。</summary>
    internal override void OnGotFocus() {
        ImeBridge.SetFocused(this);
    }

    /// <summary>
    /// 键盘消费（override）：光标/编辑键经 ImeBridge native 通道已分发
    /// TextBoxController，此处声明消费——阻止 FocusManager 将方向键误判为
    /// 焦点导航（双路由根治，InputElement.OnKeyDown 契约）。
    /// </summary>
    internal override bool OnKeyDown(int virtualKey, int shiftDown) {
        if (virtualKey == FocusManager.VirtualKeyLeft()
            || virtualKey == FocusManager.VirtualKeyUp()
            || virtualKey == FocusManager.VirtualKeyRight()
            || virtualKey == FocusManager.VirtualKeyDown()
            || virtualKey == FocusManager.VirtualKeyHome()
            || virtualKey == FocusManager.VirtualKeyEnd()
            || virtualKey == FocusManager.VirtualKeyBackspace()
            || virtualKey == FocusManager.VirtualKeyDelete()) {
            return true;
        }
        return false;
    }

    /// <summary>刷新平台 mirror（Text/composition/caret/focus/selection）。</summary>
    public void SyncMirrorText() {
        if (_mirrorHandle == 0) {
            return;
        }
        string cur = this.Text;
        if (cur == null) {
            cur = "";
        }
        string comp = this.CompositionText;
        if (comp == null) {
            comp = "";
        }
        WindowHost.ElementSetString(_mirrorHandle, "Text", cur);
        WindowHost.ElementSetString(_mirrorHandle, "CompositionText", comp);
        WindowHost.ElementSetNumber(_mirrorHandle, "CaretIndex", (double)this.CaretIndex);
        WindowHost.ElementSetNumber(_mirrorHandle, "SelectionStart", (double)this.SelectionStart);
        WindowHost.ElementSetNumber(_mirrorHandle, "SelectionLength", (double)this.SelectionLength);
        int focused = _isFocused ? 1 : 0;
        WindowHost.ElementSetBool(_mirrorHandle, "IsFocused", focused);
        // 按需渲染契约：Text/composition/caret/focus/selection 均为视觉状态，
        // 镜像更新后必须标脏——否则空闲帧 WaitEvents(-1) 阻塞，键入永不重绘。
        FramePump.Invalidate();
    }

    /// <summary>通知平台 IME 焦点与候选窗锚点（rt_ui_ime_set_focus / set_candidate_rect）。</summary>
    public void ApplyImeFocus() {
        if (!_isFocused || _mirrorHandle == 0) {
            return;
        }
        WindowHost.ImeSetFocus(_mirrorHandle);
        double fontSize = this.FontSize;
        if (fontSize <= 0.0) {
            fontSize = InputMetrics.FontSizeFallback;
        }
        // Caret-anchored DIP; Win32 IMM multiplies dpi_scale to physical client pixels.
        double caretAdv = InputMetrics.PenOriginX;
        if (TextMeasuring.IsAvailable()) {
            string before = this.CaretPrefix();
            LayoutSize sz = TextMeasuring.Current.MeasureText(
                before, fontSize, 0.0, 0.0, this.FontFamily, this.FontWeight);
            caretAdv = InputMetrics.PenOriginX + sz.Width;
        }
        int x = (int)(this.LayoutX + caretAdv);
        int y = (int)this.LayoutY;
        LayoutSize rs = this.RenderSize;
        double rsW = InputMetrics.MinWidth;
        double rsH = InputMetrics.MinHeight;
        if (rs != null) {
            rsW = rs.Width;
            rsH = rs.Height;
        }
        int w = (int)rsW;
        int h = (int)rsH;
        if (w <= 0) {
            w = (int)InputMetrics.MinWidth;
        }
        if (h <= 0) {
            h = (int)InputMetrics.MinHeight;
        }
        WindowHost.ImeSetCandidateRect(_mirrorHandle, x, y, w, h);
    }

    string CaretPrefix() {
        string cur = this.Text;
        if (cur == null) {
            cur = "";
        }
        string comp = this.CompositionText;
        int idx = _model.Caret;
        if (idx < 0) {
            idx = 0;
        }
        if (idx > cur.Length) {
            idx = cur.Length;
        }
        string before = cur.Substring(0, idx);
        if (comp != null && comp.Length > 0) {
            before = before + comp;
        }
        return before;
    }

    string BuildDisplayString() {
        string cur = this.Text;
        if (cur == null) {
            cur = "";
        }
        int idx = _model.Caret;
        if (idx < 0) {
            idx = 0;
        }
        if (idx > cur.Length) {
            idx = cur.Length;
        }
        string comp = this.CompositionText;
        if (comp == null || comp == "") {
            return cur;
        }
        return cur.Substring(0, idx) + comp + cur.Substring(idx);
    }

    protected override LayoutSize MeasureOverride(LayoutSize availableSize) {
        double fontSize = this.FontSize;
        if (fontSize <= 0.0) {
            fontSize = InputMetrics.FontSizeFallback;
        }
        LayoutSize est = LayoutHelper.EstimateTextSize(
            this.BuildDisplayString(), fontSize, InputMetrics.PadX, InputMetrics.PadY,
            this.FontFamily, this.FontWeight);
        double w = est.Width;
        double h = est.Height;
        if (w < InputMetrics.MinWidth) {
            w = InputMetrics.MinWidth;
        }
        if (h < InputMetrics.MinHeight) {
            h = InputMetrics.MinHeight;
        }
        double availW = availableSize.Width;
        if (availW > 0.0 && w > availW) {
            w = availW;
        }
        if (this.Width > 0.0) {
            w = this.Width;
        }
        if (this.Height > 0.0) {
            h = this.Height;
        }
        return new LayoutSize(w, h);
    }
}
