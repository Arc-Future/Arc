// 测量（EstText* / 辅助）partial 拆分。
//
// WgpuRender 的测量实现（partial 扩展）：EstTextWidth / EstTextHeight /
// CountTextLines / ElementColor / IsLayoutShell。
// 方法与私有字段跨文件共享，详见核心文件 WgpuRender.as。

namespace Arc.UI.Rendering.Wgpu;

using Arc.Collections;
using Arc.UI.Components;
using Arc.UI.Internal;
using Arc.UI.Layout;
using Arc.UI.Media;

public partial class WgpuRender {
    // ============================================================
    // ITextMetrics——布局与绘制同源入口（RFC 037 §9 / custom-fonts）
    // ============================================================

    /// <summary>
    /// 布局侧度量：委托 EstTextWidth/EstTextHeight（与 DrawText 同一 atlas / 族 / 字重）。
    /// </summary>
    public LayoutSize MeasureText(string text, double fontSize, double padX, double padY,
                                  string fontFamily, string fontWeight) {
        double fs = fontSize;
        if (fs <= 0.0) {
            fs = GlyphHeight;
        }
        int family = this.ResolveFontFamily(fontFamily);
        int weight = this.ResolveFontWeight(fontWeight);
        double w = this.EstTextWidth(text, padX, fs, family, weight);
        double h = this.EstTextHeight(text, padY, fs, family);
        return new LayoutSize(w, h);
    }

    // ============================================================
    // 文本测量——双路径：动态 atlas（真实字形度量）/ 8x16 fallback
    // ============================================================

    private double EstTextWidth(string text, double paddingX, double fontSize) {
        return this.EstTextWidth(text, paddingX, fontSize, 0, 0);
    }

    private double EstTextWidth(string text, double paddingX, double fontSize, int familyIdx) {
        return this.EstTextWidth(text, paddingX, fontSize, familyIdx, 0);
    }

    private double EstTextWidth(string text, double paddingX, double fontSize, int familyIdx, int weight) {
        if (text == null) { return paddingX; }
        if (_fontFallback) {
            // 8x16 fallback 路径（原有逻辑）
            double scale = 1.0;
            if (fontSize > 0.0) {
                scale = fontSize / GlyphHeight;
                if (scale < 0.5) { scale = 0.5; }
                if (scale > 4.0) { scale = 4.0; }
            }
            double gw = GlyphWidth * scale;
            double maxAdv = 0.0;
            double lineAdv = 0.0;
            int len = text.Length;
            for (int i = 0; i < len; i++) {
                char ch = text[i];
                if (ch == '\n') {
                    if (lineAdv > maxAdv) { maxAdv = lineAdv; }
                    lineAdv = 0.0;
                } else if (ch != '\r') {
                    double units = ((int)ch) > 126 ? 2.0 : 1.0;
                    lineAdv += units * gw;
                }
            }
            if (lineAdv > maxAdv) { maxAdv = lineAdv; }
            return maxAdv + paddingX;
        }
        // 动态 atlas 路径：真实字形 advance 度量（按 family + weight 选面）
        // per-size bucket：与 DrawText 同一物理像素高度，保证测量 == 渲染步进。
        double physSize = (fontSize > 0.0 ? fontSize : GlyphHeight) * _dpiScale;
        double maxAdv = 0.0;
        double lineAdv = 0.0;
        int len = text.Length;
        int i = 0;
        while (i < len) {
            int cp = 0;
            int n = this.Utf8DecodeAt(text, i, out cp);
            if (n == 0) { break; }
            if (cp == '\n') {
                if (lineAdv > maxAdv) { maxAdv = lineAdv; }
                lineAdv = 0.0;
            } else if (cp != '\r') {
                double u0 = 0.0;
                double v0 = 0.0;
                double u1 = 0.0;
                double v1 = 0.0;
                double adv = 0.0;
                double xoff = 0.0;
                double yoff = 0.0;
                double gw = 0.0;
                double gh = 0.0;
                int r = wgpu_native.wgpu_font_atlas_lookup_glyph(
                    _fontAtlas, familyIdx, weight, cp, physSize,
                    out u0, out v0, out u1, out v1,
                    out adv, out xoff, out yoff, out gw, out gh);
                if (r >= 0) {
                    // 成功（已缓存或新增），advance 是 size_px 物理像素，转 DIP
                    lineAdv += adv / _dpiScale;
                } else {
                    // 字形缺失：用半角空格宽度近似
                    lineAdv += GlyphWidth * (fontSize / GlyphHeight);
                }
            }
            i += n;
        }
        if (lineAdv > maxAdv) { maxAdv = lineAdv; }
        return maxAdv + paddingX;
    }

    private double EstTextHeight(string text, double paddingY, double fontSize) {
        return this.EstTextHeight(text, paddingY, fontSize, 0);
    }

    private double EstTextHeight(string text, double paddingY, double fontSize, int familyIdx) {
        double fs = (fontSize > 0.0) ? fontSize : GlyphHeight;
        if (_fontFallback) {
            // 8x16 fallback 路径（原有逻辑）
            double scale = fs / GlyphHeight;
            if (scale < 0.5) { scale = 0.5; }
            if (scale > 4.0) { scale = 4.0; }
            double gh = GlyphHeight * scale;
            if (text == null) { return gh + paddingY; }
            int lines = 1;
            int len = text.Length;
            for (int i = 0; i < len; i++) {
                if (text[i] == '\n') { lines++; }
            }
            return (double)lines * gh + paddingY;
        }
        // 动态 atlas 路径：真实行高 = (ascent - descent + line_gap) DIP（按 family 度量）
        double ascent = wgpu_native.wgpu_font_atlas_get_family_ascent(_fontAtlas, familyIdx);
        double descent = wgpu_native.wgpu_font_atlas_get_family_descent(_fontAtlas, familyIdx);
        double lineGap = wgpu_native.wgpu_font_atlas_get_family_line_gap(_fontAtlas, familyIdx);
        double lineH = (ascent - descent + lineGap) * fs / _atlasBasePx;
        if (text == null) { return lineH + paddingY; }
        int lines = 1;
        int len = text.Length;
        for (int i = 0; i < len; i++) {
            if (text[i] == '\n') { lines++; }
        }
        return (double)lines * lineH + paddingY;
    }

    // ============================================================
    // FontFamily 辅助（阶段二）：名称 ↔ 索引解析 + 注册
    // ============================================================

    /// <summary>按名称解析 family 索引；未注册/失败回退默认族（0）。未知名经 FontManager 一次性可观察诊断。</summary>
    private int ResolveFontFamily(string name) {
        if (_fontFallback || _fontAtlas == null) { return 0; }
        if (name == null || name.Length == 0) { return 0; }
        int idx = wgpu_native.wgpu_font_atlas_get_family_index(_fontAtlas, name);
        if (idx >= 0) { return idx; }
        if (Application.Current != null) {
            Application.Current.Fonts.WarnUnknownFamily(name);
        }
        return 0;
    }

    /// <summary>
    /// 内部适配：由 <see cref="FontManager"/> 调用。用户正道仅 <c>Application.Fonts.RegisterFamily</c>。
    /// chain: 仅 Normal，或 FontManager 的 <c>normal|bold</c>（单 '|' → Bold 面）。
    /// 返回 family 索引（>=1）；失败 -1。
    /// </summary>
    internal int RegisterFontFamily(string name, string chain) {
        if (_fontFallback || _fontAtlas == null) { return -1; }
        if (name == null || chain == null) { return -1; }
        return wgpu_native.wgpu_font_atlas_add_family(_fontAtlas, name, chain);
    }

    /// <summary>
    /// FontWeight DP → atlas weight：1 = Bold 面，0 = Normal 面。
    /// 仅 "Bold" / "700" 选 Bold；其余字面量回退 Normal（不假装第三套字面）。
    /// </summary>
    private int ResolveFontWeight(string weight) {
        if (weight == null) { return 0; }
        if (weight == "Bold" || weight == "700") { return 1; }
        return 0;
    }

    /// <summary>测量单/多行文本行数。</summary>
    private int CountTextLines(string text) {
        if (text == null || text.Length == 0) { return 1; }
        int lines = 1;
        for (int i = 0; i < text.Length; i++) {
            if (text[i] == '\n') { lines++; }
        }
        return lines;
    }

    private Color ElementColor(long handle, string name, Color def) {
        string s = WindowHost.ElementGetString(handle, name, def.ToHex());
        if (s == null || s.Length == 0) {
            return def;
        }
        return Color.Parse(s);
    }

    private bool IsLayoutShell(string type) {
        return type == ElGrid
            || type == ElDockPanel
            || type == ElWrapPanel
            || type == ElCanvas
            || type == ElListView;
    }
}
