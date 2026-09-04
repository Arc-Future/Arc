// RFC 037 M-CE1: line index facade over TextBuffer C-side index.

namespace Arc.UI.Editing;

/// <summary>
/// 行 byte-offset 索引门面（≤64 MB eager；更大 256 KB 分块懒建 · Draft）。
/// </summary>
/// <remarks>索引存储于 C <c>rt_editor</c>；Arc 层仅暴露 Ensure 与计数。见 RFC 037。</remarks>
internal class LineIndex {
    private TextBuffer _buffer;

    public LineIndex(TextBuffer buffer) {
        _buffer = buffer;
    }

    /// <summary>总行数。</summary>
    public int LineCount {
        get {
            if (_buffer == null) {
                return 0;
            }
            return _buffer.LineCount;
        }
    }

    /// <summary>确保 [firstLine, lastLine] 对应块已扫描（视口 ± overscan 调用）。</summary>
    public bool EnsureRange(int firstLine, int lastLine) {
        if (_buffer == null) {
            return false;
        }
        return _buffer.EnsureLines(firstLine, lastLine);
    }
}
