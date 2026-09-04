// RFC 037 M-CE1: viewport math for virtualized CodeEditor rendering.

namespace Arc.UI.Editing;

using Arc.UI.Layout;

/// <summary>
/// 编辑器视口：滚动偏移 → 可见行范围 + 算术 Extent（M-CE1 硬约束）。
/// </summary>
/// <remarks>
/// 禁止为每行创建 Visual/Text 子元素；ExtentHeight = LineCount × LineHeight。
/// 见 RFC 037 · 短研 2c4a16f1。
/// </remarks>
internal class EditorViewport {
    /// <summary>可见区外额外 materialize 行数（上下各 N）。</summary>
    public const int OverscanLines = 5;

    private int _firstVisibleLine;
    private int _lastVisibleLine;
    private double _subLineOffset;
    private double _lineHeight;
    private double _extentHeight;

    public EditorViewport() {
        _firstVisibleLine = 0;
        _lastVisibleLine = 0;
        _subLineOffset = 0.0;
        _lineHeight = 0.0;
        _extentHeight = 0.0;
    }

    /// <summary>首条 materialize 行（含 overscan）。</summary>
    public int FirstVisibleLine {
        get { return _firstVisibleLine; }
    }

    /// <summary>末条 materialize 行（含 overscan）。</summary>
    public int LastVisibleLine {
        get { return _lastVisibleLine; }
    }

    /// <summary>视口内首行亚像素偏移（scroll % lineHeight）。</summary>
    public double SubLineOffset {
        get { return _subLineOffset; }
    }

    /// <summary>当前行高（px）。</summary>
    public double LineHeight {
        get { return _lineHeight; }
    }

    /// <summary>算术内容总高（LineCount × LineHeight）。</summary>
    public double ExtentHeight {
        get { return _extentHeight; }
    }

    /// <summary>
    /// 根据滚动偏移与视口高度更新可见行范围。
    /// 行高经 <see cref="LayoutHelper.EstimateLineHeight"/> 与 atlas 同源。
    /// </summary>
    public void Update(double scrollOffsetY, double viewportHeight, int lineCount,
                       double fontSize, string fontFamily, string fontWeight) {
        _lineHeight = LayoutHelper.EstimateLineHeight(fontSize, fontFamily, fontWeight);

        if (lineCount <= 0) {
            _firstVisibleLine = 0;
            _lastVisibleLine = -1;
            _subLineOffset = 0.0;
            _extentHeight = 0.0;
            return;
        }

        _extentHeight = (double)lineCount * _lineHeight;

        int first = (int)(scrollOffsetY / _lineHeight) - OverscanLines;
        if (first < 0) {
            first = 0;
        }

        int visibleRows = (int)(viewportHeight / _lineHeight) + 2;
        int last = first + visibleRows + OverscanLines * 2;
        if (last >= lineCount) {
            last = lineCount - 1;
        }
        if (last < first) {
            last = first;
        }

        _firstVisibleLine = first;
        _lastVisibleLine = last;
        _subLineOffset = scrollOffsetY - (double)first * _lineHeight;
    }
}
