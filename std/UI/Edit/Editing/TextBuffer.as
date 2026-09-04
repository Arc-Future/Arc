// RFC 037 M-CE1: Piece-table document buffer (C core + Arc facade).
// Architecture: 短研 2c4a16f1 — mmap original + add buffer; NOT rope; NOT full string.

namespace Arc.UI.Editing;

/// <summary>
/// 可编辑文本文档缓冲（Piece Table · Draft · M-CE1）。
/// </summary>
/// <remarks>
/// <list type="bullet">
/// <item><see cref="OpenPath"/> 经 mmap，禁止 ReadAllText 全量加载。</item>
/// <item><see cref="LineText"/> 仅 materialize 单行。</item>
/// <item>编辑追加 add buffer，不拼接 GB 级 string。</item>
/// </list>
/// 权威：见 RFC 037。
/// </remarks>
public class TextBuffer {
    private long _handle;

    /// <summary>空文档。</summary>
    [Builtin(ABI = "rt_editor_create_empty")]
    public TextBuffer() {
        _handle = 0;
    }

    /// <summary>文档字节长度。</summary>
    [Builtin(ABI = "rt_editor_length")]
    public long Length { get; }

    /// <summary>总行数（含末行无换行）。</summary>
    [Builtin(ABI = "rt_editor_line_count")]
    public int LineCount { get; }

    /// <summary>是否由 mmap 原稿支撑（OpenPath 路径）。</summary>
    [Builtin(ABI = "rt_editor_is_mmap_backed")]
    public bool IsMmapBacked { get; }

    /// <summary>
    /// 从路径 mmap 打开（静态工厂）。大文件零拷贝；禁止 ReadAllText。
    /// </summary>
    [Builtin(ABI = "rt_editor_open_path")]
    public static TextBuffer OpenPath(string path) {
        return null;
    }

    /// <summary>替换为小文本（内存路径；非 GB 打开）。</summary>
    [Builtin(ABI = "rt_editor_set_text")]
    public bool SetText(string text) {
        return false;
    }

    /// <summary>读取单行文本（malloc 单行；视口虚拟化用）。</summary>
    [Builtin(ABI = "rt_editor_line_text")]
    public string LineText(int lineIndex) {
        return "";
    }

    /// <summary>确保行索引覆盖 [firstLine, lastLine]（大文件分块懒建）。</summary>
    [Builtin(ABI = "rt_editor_ensure_lines")]
    public bool EnsureLines(int firstLine, int lastLine) {
        return false;
    }

    /// <summary>在字节偏移处插入 UTF-8 文本。</summary>
    [Builtin(ABI = "rt_editor_insert")]
    public bool Insert(long byteOffset, string text) {
        return false;
    }

    /// <summary>删除字节范围。</summary>
    [Builtin(ABI = "rt_editor_delete")]
    public bool Delete(long byteOffset, long byteLength) {
        return false;
    }

    /// <summary>释放 C 侧文档。</summary>
    [Builtin(ABI = "rt_editor_destroy")]
    public void Dispose() {
    }
}
