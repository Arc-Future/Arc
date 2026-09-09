// RFC 037 M-CE1 · RFC 037 标杆：CodeEditor 视口虚拟化。
//
// M-CE1 硬约束（RFC 037 §4）：
//   - 视口虚拟化：只 materialize 可见行 ± overscan 至 DrawList
//   - 禁止为每行创建 Text/Visual 子元素
//   - ExtentHeight 算术化（LineCount × LineHeight）
//   - OpenPath 经 mmap piece-table；禁止 ReadAllText / VisualHost 1GB 宿主
//
// 上屏路径：BuildFrameDrawList → FrameDrawListRouter → WgpuRender.RenderTree
// ExecuteDrawList（局部坐标 + 布局原点偏移 + PushClip）。
//
// 权威：RFC 037 §4（docs/rfc/037-ui.md）· std/UI/Core/COMPONENTS.md M-CE1 立宪

namespace Arc.UI.Components;

using Arc.UI;
using Arc.UI.Editing;
using Arc.UI.Internal;
using Arc.UI.Layout;
using Arc.UI.Rendering;
using Arc.UI.Styling;

/// <summary>
/// 大文档代码编辑器（Piece Table + 视口虚拟化 · M-CE1）。
/// </summary>
public class CodeEditor : Control, IFrameDrawListProvider {
    // 静态 DP 元数据保留供 ARML/typecheck；M-CE1 控制台 smoke 走字段后备（RegisterProperty __sinit 挂账）。
    public static DependencyProperty<double> VerticalOffsetProperty =
        RegisterProperty<double>(nameof(VerticalOffset), typeof(CodeEditor), 0.0);

    public static DependencyProperty<string> DocumentPathProperty =
        RegisterProperty<string>(nameof(DocumentPath), typeof(CodeEditor), "");

    TextBuffer _document;
    LineIndex _lineIndex;
    EditorViewport _viewport;
    DrawList _frameList;
    long _mirrorHandle;

    double _verticalOffset;
    string _documentPath;
    double _fontSize;
    double _renderHeight;

    public CodeEditor() {
        this.Type = typeof(CodeEditor);
        this.TypeName = "CodeEditor";
        _verticalOffset = 0.0;
        _documentPath = "";
        _fontSize = 14.0;
        _renderHeight = 480.0;
        _mirrorHandle = 0;
        _document = new TextBuffer();
        _lineIndex = new LineIndex(_document);
        _viewport = new EditorViewport();
        _frameList = new DrawList();
    }

    /// <summary>垂直滚动偏移（px）。ScrollView 外壳可读此 DP。</summary>
    public double VerticalOffset {
        get { return _verticalOffset; }
        set {
            _verticalOffset = value;
            this.InvalidateVisual();
        }
    }

    /// <summary>当前打开路径（ARML 绑定 / 诊断）。</summary>
    public string DocumentPath {
        get { return _documentPath; }
        set { _documentPath = value; }
    }

    /// <summary>底层文档缓冲（Piece Table）。</summary>
    public TextBuffer Document {
        get { return _document; }
    }

    /// <summary>算术内容总高（ScrollView Extent；不 Measure 子内容）。</summary>
    public double ContentExtentHeight {
        get {
            _viewport.Update(
                _verticalOffset,
                _renderHeight,
                _document.LineCount,
                _fontSize,
                this.FontFamily,
                this.FontWeight);
            return _viewport.ExtentHeight;
        }
    }

    /// <summary>IFrameDrawListProvider：已绑定平台句柄。</summary>
    public long DrawListMirrorHandle {
        get { return _mirrorHandle; }
    }

    /// <summary>mmap 打开路径；禁止 ReadAllText。</summary>
    public bool OpenPath(string path) {
        if (path == null || path.Length == 0) {
            return false;
        }
        TextBuffer opened = TextBuffer.OpenPath(path);
        if (opened == null) {
            return false;
        }
        _document.Dispose();
        _document = opened;
        _lineIndex = new LineIndex(_document);
        _documentPath = path;
        this.InvalidateVisual();
        return true;
    }

    /// <summary>设置小文本（非 GB 路径）。</summary>
    public bool SetText(string text) {
        bool ok = _document.SetText(text);
        if (ok) {
            this.InvalidateVisual();
        }
        return ok;
    }

    /// <summary>平台镜像建树登记：句柄 → FrameDrawListRouter，供 RenderTree 回查。</summary>
    public void BindDrawListMirror(long platformHandle) {
        _mirrorHandle = platformHandle;
        FrameDrawListRouter.Register(platformHandle, this);
    }

    /// <summary>
    /// 虚拟化渲染：仅可见行 ± overscan → DrawList DrawText。
    /// </summary>
    public DrawList RenderVirtualizedLines() {
        return this.BuildFrameDrawList();
    }

    /// <summary>IFrameDrawListProvider：物化本帧可见行（局部坐标）。</summary>
    public DrawList BuildFrameDrawList() {
        _frameList.Clear();

        double viewportH = _renderHeight;
        if (viewportH <= 0.0) {
            viewportH = 480.0;
        }

        int lineCount = _document.LineCount;
        _viewport.Update(_verticalOffset, viewportH, lineCount, _fontSize,
                         this.FontFamily, this.FontWeight);

        int first = _viewport.FirstVisibleLine;
        int last = _viewport.LastVisibleLine;
        if (lineCount == 0 || last < first) {
            return _frameList;
        }

        _lineIndex.EnsureRange(first, last);

        double lineH = _viewport.LineHeight;
        double yBase = -_viewport.SubLineOffset;
        // 环境前景：有本地/继承用 DP；未设走活动主题 Text.Primary（与 TextBlock 同契约）。
        string fg = this.Foreground;
        if (!this.HasAmbientValue(Control.ForegroundProperty.Id)) {
            if (Application.Current != null) {
                string textPrimary = Application.Current.ResolveColor(BuiltInTheme.TextPrimary);
                if (textPrimary != null && textPrimary.Length > 0) {
                    fg = textPrimary;
                }
            }
        }
        string bg = "#00000000";

        for (int line = first; line <= last; line++) {
            string text = _document.LineText(line);
            double y = yBase + (double)(line - first) * lineH;
            _frameList.AddDrawText(4.0, y, text, _fontSize, fg, bg);
        }

        return _frameList;
    }

    /// <summary>最近一次 RenderVirtualizedLines 写入的命令数（M-CE1 smoke）。</summary>
    public int LastDrawCommandCount {
        get { return _frameList.Count; }
    }

    /// <summary>Measure：固定视口尺寸，不用内容总高驱动 layout。</summary>
    protected override LayoutSize MeasureOverride(LayoutSize availableSize) {
        double w = availableSize.Width;
        double h = availableSize.Height;
        if (w <= 0.0) {
            w = 720.0;
        }
        if (h <= 0.0) {
            h = 480.0;
        }
        return new LayoutSize(w, h);
    }

    /// <summary>Arrange：记录渲染尺寸供视口计算（须 void，与 FrameworkElement 契约一致）。</summary>
    protected override void ArrangeOverride(LayoutSize finalSize) {
        if (finalSize.Height > 0.0) {
            _renderHeight = finalSize.Height;
        }
    }

    public void BindEditorFocus() {
        EditorInputRouter.RegisterEditor(this);
    }

    public void ReleaseEditorFocus() {
        EditorInputRouter.UnregisterEditor(this);
    }

    public void InvalidateVisual() {
        this.BuildFrameDrawList();
        FramePump.Invalidate();
    }
}