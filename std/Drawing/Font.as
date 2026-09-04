// RFC 029 M6：字体加载与度量（vendored stb_truetype 经 rt_image_font_* ABI 直射）。
//
// 设计（对齐 RFC 029 §1.4 ④ + §3 M6）：
//   - Font = 持 long _handle（rt_font_handle 整数形态）的 regular class；
//   - `[Builtin]` 私有静态辅助方法（_Load/_Metrics/_Measure/_Glyph/_Free）经
//     codegen 拦截直射 `rt_image_font_*`（`Font::_X` `::` 分隔，AesGcm 先例）；
//   - 实例方法 LineHeight / MeasureTextWidth / Glyph 为真实 Arc 方法体，
//     包内 Bitmap.Drawing（partial Bitmap）经 internal Glyph 合成文本。
//
// 诚实边界：stb_truetype 声明不做不可信字体安全校验（VENDOR.md M6 注记）；
// 仅 TrueType/OpenType 轮廓，不支持位图字体 / 彩色字体。

namespace Arc.Drawing;

using Arc;
using Arc.IO;

/// <summary>
/// 字体（RFC 029 M6）：加载 TTF/OTF → 度量 / 步进度量 / 字形单通道灰度位图。
/// Dispose 释放原生字体句柄。
/// </summary>
public class Font : IDisposable {
    private long _handle;

    [Builtin(ABI = "rt_image_font_load")]
    private static long _Load(byte[] ttf, float size) { return 0; }

    [Builtin(ABI = "rt_image_font_metrics")]
    private static int _Metrics(long handle, out float ascent, out float descent, out float lineGap) { return -1; }

    [Builtin(ABI = "rt_image_font_measure")]
    private static float _Measure(long handle, string text) { return 0.0; }

    [Builtin(ABI = "rt_image_font_glyph")]
    private static int _Glyph(long handle, int codepoint, byte[] bitmap, out int w, out int h, out float xoff, out float yoff) { return -1; }

    [Builtin(ABI = "rt_image_font_free")]
    private static void _Free(long handle) { }

    /// <summary>加载 TTF/OTF 文件（size = 像素高度）。文件缺失/损坏抛 IOException。</summary>
    public Font(string path, float size) {
        if (path == null) {
            throw new ArgumentNullException("path");
        }
        byte[] ttf = File.ReadAllBytes(path);
        if (ttf == null || ttf.Length == 0) {
            throw new IOException("font file missing or empty: " + path);
        }
        long h = Font._Load(ttf, size);
        if (h == 0) {
            throw new IOException("failed to load font: " + path);
        }
        _handle = h;
    }

    /// <summary>行高（ascent − descent + line_gap，像素域；加载失败 0）。</summary>
    public float LineHeight {
        get {
            float a = 0.0;
            float d = 0.0;
            float g = 0.0;
            int rc = Font._Metrics(_handle, out a, out d, out g);
            if (rc != 0) {
                return 0.0;
            }
            return a - d + g;
        }
    }

    /// <summary>步进度量（逐字符 advance + kerning 累加，UTF-8 输入）。</summary>
    public float MeasureTextWidth(string text) {
        if (text == null) {
            return 0.0;
        }
        return Font._Measure(_handle, text);
    }

    /// <summary>字形单通道灰度位图（alpha 覆盖；bitmap=null 查询尺寸）。
    /// 返回 0 成功 / 非零失败。</summary>
    internal int Glyph(int codepoint, byte[] bitmap, out int w, out int h, out float xoff, out float yoff) {
        w = 0;
        h = 0;
        xoff = 0.0;
        yoff = 0.0;
        return Font._Glyph(_handle, codepoint, bitmap, out w, out h, out xoff, out yoff);
    }

    /// <summary>释放原生字体句柄（幂等）。</summary>
    public void Dispose() {
        if (_handle != 0) {
            Font._Free(_handle);
            _handle = 0;
        }
    }
}
