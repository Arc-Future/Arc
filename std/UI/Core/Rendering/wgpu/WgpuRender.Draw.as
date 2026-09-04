// RFC 037 P3 · 绘制图元（partial 拆分）。
//
// WgpuRender 的绘制图元实现（partial 扩展）：DrawRect / DrawLine / DrawText /
// ExecuteDrawList / UTF-16 解码 / 颜色解析辅助。方法与私有字段跨文件共享，
// 详见核心文件 WgpuRender.as（`public partial class WgpuRender`）。

namespace Arc.UI.Rendering.Wgpu;

using Arc.Collections;
using Arc.UI.Components;
using Arc.UI.Internal;

public partial class WgpuRender {
    /// <summary>绘制直角填充矩形（radius=0，stroke=0）。</summary>
    public void DrawRect(double x, double y, double width, double height, Color fillColor) {
        this.DrawSurfaceFill(x, y, width, height, fillColor, fillColor,
                             0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    }

    /// <summary>绘制圆角填充矩形（stroke=0）。radius 超过 min(w,h)/2 时为胶囊/圆。</summary>
    public void DrawRoundedRect(double x, double y, double width, double height,
                                double radius, Color fillColor) {
        this.DrawSurfaceFill(x, y, width, height, fillColor, fillColor,
                             0.0, 0.0, 0.0, 0.0, radius, 0.0);
    }

    /// <summary>绘制圆角描边环（stroke=thickness）。radius 与厚度均为 DIP。</summary>
    public void DrawRoundedBorder(double x, double y, double width, double height,
                                  double radius, double thickness, Color color) {
        this.DrawSurfaceFill(x, y, width, height, color, color,
                             0.0, 0.0, 0.0, 0.0, radius, thickness);
    }

    /// <summary>
    /// 统一表面填充：同一 <c>wgpu_batch_rect_write</c>（80 字节）承载纯色/渐变 × 直角/圆角 × 填充/描边。
    /// 纯色：c1=c0 且渐变轴长度为零；渐变：c0≠c1 + 非零轴；radius/stroke 经 SDF 裁剪。
    /// 禁止「直角渐变填充 + 圆角描边」冒充圆角。
    /// </summary>
    private void DrawSurfaceFill(double x, double y, double width, double height,
                                 Color startColor, Color endColor,
                                 double sx, double sy, double ex, double ey,
                                 double radius, double stroke) {
        if (!_initialized || _pass == null) {
            return;
        }
        if (_uniformOffset + UniformSlotSize > UniformBufferSize) {
            _overflowDropped++;
            return;
        }
        // 类型化 Color 直接到位：sRGB 分量写 uniform 前转线性空间（alpha 不做 gamma）。
        double c0r = this.SrgbToLinear(startColor.R);
        double c0g = this.SrgbToLinear(startColor.G);
        double c0b = this.SrgbToLinear(startColor.B);
        double c0a = startColor.A;
        double c1r = this.SrgbToLinear(endColor.R);
        double c1g = this.SrgbToLinear(endColor.G);
        double c1b = this.SrgbToLinear(endColor.B);
        double c1a = endColor.A;
        wgpu_native.wgpu_batch_rect_write(
            _staging, _uniformOffset,
            x * _dpiScale, y * _dpiScale,
            width * _dpiScale, height * _dpiScale,
            c0r, c0g, c0b, c0a,
            c1r, c1g, c1b, c1a,
            sx, sy, ex, ey,
            radius * _dpiScale, stroke * _dpiScale,
            _surfaceWidth, _surfaceHeight
        );
        _cmdOffset.Add(_uniformOffset);
        _cmdPipeline.Add(0);  // 0 = 统一表面填充管线
        this.RecordCommandScissor();
        _uniformOffset += UniformSlotSize;
    }

    /// <summary>
    /// 线性渐变表面填充：c0→c1 沿起止归一化轴插值，radius 经同一 SDF 裁剪（可与描边叠用）。
    /// startColor/endColor 为类型化 Color（sRGB 分量）；sx/sy/ex/ey 为 0-1 归一化；radius 为 DIP。
    /// </summary>
    public void DrawLinearGradient(double x, double y, double w, double h,
                                   Color startColor, Color endColor,
                                   double sx, double sy, double ex, double ey,
                                   double radius) {
        this.DrawSurfaceFill(x, y, w, h, startColor, endColor, sx, sy, ex, ey, radius, 0.0);
    }

    /// <summary>
    /// RFC 037 §3.5 浮层阴影（Shadow.Surface）：在核心矩形（表面）后方绘制软阴影。
    /// 阴影 quad = 核心矩形向四周扩大 margin=3*blur（高斯尾部）并偏移 offsetY；
    /// 片元按到核心矩形边界的高斯衰减输出半透明黑阴影。blur/offsetY 为 DIP。
    /// </summary>
    public void DrawSurfaceShadow(double cx, double cy, double cw, double ch,
                                  double radius, double blur, double offsetY, double alpha) {
        if (!_initialized || _pass == null) {
            return;
        }
        if (blur <= 0.0) {
            return;
        }
        if (_uniformOffset + UniformSlotSize > UniformBufferSize) {
            return;
        }
        double margin = 3.0 * blur;
        double qx = cx - margin;
        double qy = cy - margin + offsetY;
        double qw = cw + 2.0 * margin;
        double qh = ch + 2.0 * margin;
        // quad/核心矩形与 blur 均换算到物理像素（shader 内距离单位为物理 px）。
        wgpu_native.wgpu_batch_shadow_write(
            _staging, _uniformOffset,
            qx * _dpiScale, qy * _dpiScale, qw * _dpiScale, qh * _dpiScale,
            cx * _dpiScale, cy * _dpiScale, cw * _dpiScale, ch * _dpiScale,
            radius, blur * _dpiScale, alpha,
            _surfaceWidth, _surfaceHeight);
        _cmdOffset.Add(_uniformOffset);
        _cmdPipeline.Add(2);  // 2 = shadow pipeline
        this.RecordCommandScissor();
        _uniformOffset += UniformSlotSize;
    }

    /// <summary>
    /// 绘制纹理表面（RFC 037 references/texture-surface）。
    /// 采样动态纹理的 uv 矩形到目标矩形。复用文本 uniform 布局与 staging 写入器
    /// （wgpu_batch_text_write）——图像 shader 直出纹理色，uniform 的 rgb 作 tint（默认 1）。
    /// </summary>
    public void DrawTexture(int textureId, double x, double y, double w, double h,
                            double u0, double v0, double u1, double v1, double alpha) {
        if (!_initialized || _pass == null) {
            return;
        }
        if (this.GetTextureBindGroup(textureId) == null) {
            return;
        }
        if (_uniformOffset + UniformSlotSize > UniformBufferSize) {
            _overflowDropped++;
            return;
        }
        wgpu_native.wgpu_batch_text_write(
            _staging, _uniformOffset,
            x * _dpiScale, y * _dpiScale, w * _dpiScale, h * _dpiScale,
            u0, v0, u1, v1,
            1.0, 1.0, 1.0, alpha,
            _surfaceWidth, _surfaceHeight);
        _cmdOffset.Add(_uniformOffset);
        _cmdPipeline.Add(3);  // 3 = image pipeline
        _cmdTexture.Add(textureId);
        this.RecordCommandScissor();
        _uniformOffset += UniformSlotSize;
    }

    // ============================================================
    // UTF-8 解码辅助：Arc `string` 即 UTF-8（`s[i]` 返回单个 UTF-8 码元），
    // 逐字符解码 → Unicode codepoint。纯算术实现（Arc 位运算 lowering 未实现），
    // 与 runtime-drawing/rt_font.c 的 rt_font_utf8_decode 语义一致。
    // ============================================================

    /// <summary>从 UTF-8 字符串 s 的位置 i 解码一个 Unicode codepoint。
    /// 返回消耗的 UTF-8 字节数（1-4），cp 输出 codepoint；非法序列回退单字节。</summary>
    private int Utf8DecodeAt(string s, int i, out int cp) {
        cp = 0;
        if (s == null || i < 0 || i >= s.Length) { return 0; }
        int b0 = (int)s[i];
        if (b0 < 0x80) { cp = b0; return 1; }
        if (b0 < 0xC0) { cp = b0; return 1; }  // 非法连续字节，回退单字节
        if (b0 < 0xE0) {
            if (i + 1 >= s.Length) { cp = b0; return 1; }
            int b1 = (int)s[i + 1];
            if (b1 < 0x80 || b1 >= 0xC0) { cp = b0; return 1; }
            cp = (b0 % 32) * 64 + (b1 % 64);
            return 2;
        }
        if (b0 < 0xF0) {
            if (i + 2 >= s.Length) { cp = b0; return 1; }
            int b1 = (int)s[i + 1];
            int b2 = (int)s[i + 2];
            if (b1 < 0x80 || b1 >= 0xC0 || b2 < 0x80 || b2 >= 0xC0) { cp = b0; return 1; }
            cp = (b0 % 16) * 4096 + (b1 % 64) * 64 + (b2 % 64);
            return 3;
        }
        if (b0 < 0xF8) {
            if (i + 3 >= s.Length) { cp = b0; return 1; }
            int b1 = (int)s[i + 1];
            int b2 = (int)s[i + 2];
            int b3 = (int)s[i + 3];
            if (b1 < 0x80 || b1 >= 0xC0 || b2 < 0x80 || b2 >= 0xC0 || b3 < 0x80 || b3 >= 0xC0) {
                cp = b0;
                return 1;
            }
            cp = (b0 % 8) * 262144 + (b1 % 64) * 4096 + (b2 % 64) * 64 + (b3 % 64);
            return 4;
        }
        cp = b0;
        return 1;
    }

    /// <summary>
    /// 渲染文本。优先使用动态 stb_truetype atlas（跨平台 CJK/Latin 高清字形）；
    /// 字体加载失败时回退到内置 8x16 点阵。
    /// fontSize<=0 时回退默认字号（动态 atlas 用 AtlasBasePx/dpi 映射，8x16 用 16px）。
    /// </summary>
    public void DrawText(string text, double x, double y, double fontSize,
                         Color bgColor, Color fgColor) {
        this.DrawText(text, x, y, fontSize, bgColor, fgColor, 0, 0);
    }

    /// <summary>按字体族渲染文本（familyIdx = family 索引，0 = 默认族）。</summary>
    public void DrawText(string text, double x, double y, double fontSize,
                         Color bgColor, Color fgColor, int familyIdx) {
        this.DrawText(text, x, y, fontSize, bgColor, fgColor, familyIdx, 0);
    }

    /// <summary>按字体族 + 字重渲染（weight: 0=Normal，1=Bold）。</summary>
    public void DrawText(string text, double x, double y, double fontSize,
                         Color bgColor, Color fgColor, int familyIdx, int weight) {
        if (!_initialized || _pass == null) {
            return;
        }
        if (text == null) { text = ""; }

        // 统一默认字号
        double fs = (fontSize > 0.0) ? fontSize : (_fontFallback ? GlyphHeight : 14.0);

        // 类型化前景色：sRGB 分量写 uniform 前线性化（alpha 不做 gamma）
        double fr = this.SrgbToLinear(fgColor.R);
        double fg = this.SrgbToLinear(fgColor.G);
        double fb = this.SrgbToLinear(fgColor.B);
        double fa = fgColor.A;

        if (_fontFallback) {
            // ========== 8x16 fallback：UTF-8 解码后查点阵；非 ASCII 用 tofu ==========
            double scale = fs / GlyphHeight;
            if (scale < 0.5) { scale = 0.5; }
            if (scale > 4.0) { scale = 4.0; }
            double gw = GlyphWidth * scale;
            double gh = GlyphHeight * scale;

            int lines = 1;
            double maxWidthUnits = 0.0;
            double lineWidthUnits = 0.0;
            int textLength = text.Length;
            int mi = 0;
            while (mi < textLength) {
                int mcp = 0;
                int mn = this.Utf8DecodeAt(text, mi, out mcp);
                if (mn == 0) { break; }
                mi += mn;
                if (mcp == '\n') {
                    lines++;
                    if (lineWidthUnits > maxWidthUnits) { maxWidthUnits = lineWidthUnits; }
                    lineWidthUnits = 0.0;
                } else if (mcp != '\r') {
                    double units = (mcp > 126) ? 2.0 : 1.0;
                    lineWidthUnits += units;
                }
            }
            if (lineWidthUnits > maxWidthUnits) { maxWidthUnits = lineWidthUnits; }
            double estimatedWidth = maxWidthUnits * gw + MinTextPaddingX;
            double estimatedHeight = (double)lines * gh + MinTextPaddingY;

            double bgAlpha = bgColor.A;
            if (bgAlpha > 0.001) {
                this.DrawRect(x, y, estimatedWidth, estimatedHeight, bgColor);
            }

            double gridXStart = x + MinTextPaddingX / 2.0;
            double gridYStart = y + MinTextPaddingY / 2.0;
            double gx = gridXStart;
            double gy = gridYStart;
            int i = 0;
            while (i < textLength) {
                int cp = 0;
                int n = this.Utf8DecodeAt(text, i, out cp);
                if (n == 0) { break; }
                i += n;
                if (cp == '\n') { gx = gridXStart; gy += gh; continue; }
                if (cp == '\r') { continue; }
                int glyph;
                double charGw;
                if (cp >= 32 && cp <= 126) { glyph = cp - 32; charGw = gw; }
                else { glyph = GlyphTofuIndex; charGw = gw * 2.0; }
                if (_uniformOffset + UniformSlotSize > UniformBufferSize) {
                    _overflowDropped++;
                    break;
                }

                double col = (double)(glyph % 16);
                double row = (double)(glyph / 16);
                double u0 = (col * GlyphWidth) / 128.0;
                double v0 = (row * GlyphHeight) / 96.0;
                double u1 = u0 + GlyphWidth / 128.0;
                double v1 = v0 + GlyphHeight / 96.0;

                wgpu_native.wgpu_batch_text_write(
                    _staging, _uniformOffset,
                    gx * _dpiScale, gy * _dpiScale,
                    charGw * _dpiScale, gh * _dpiScale,
                    u0, v0, u1, v1,
                    fr, fg, fb, fa,
                    _surfaceWidth, _surfaceHeight);
                _cmdOffset.Add(_uniformOffset);
                _cmdPipeline.Add(1);  // 1 = text pipeline
                this.RecordCommandScissor();
                _uniformOffset += UniformSlotSize;
                gx += charGw;
            }
            return;
        }

        // ========== 动态 stb_truetype atlas 路径 ==========
        // per-size bucket：按目标物理像素高度（fs × dpi）1:1 光栅化字形，
        // 屏幕采样零缩放——正文锐度的决定性前提（对标 glyphon/iced）。
        // 度量仍取 base 度量按 dipScale 线性换算（矢量度量缩放无损）。
        double physSize = fs * _dpiScale;
        double dipScale = fs / _atlasBasePx;
        // 按 family 取度量（0 = 默认族；越界 C 侧回退 family 0）
        double famAscent = wgpu_native.wgpu_font_atlas_get_family_ascent(_fontAtlas, familyIdx);
        double famDescent = wgpu_native.wgpu_font_atlas_get_family_descent(_fontAtlas, familyIdx);
        double famLineGap = wgpu_native.wgpu_font_atlas_get_family_line_gap(_fontAtlas, familyIdx);
        double ascentDip = famAscent * dipScale;
        double descentDip = -famDescent * dipScale;  // famDescent 为负
        double lineHeightDip = (famAscent - famDescent + famLineGap) * dipScale;

        // P2 消冗余：estimatedWidth/estimatedHeight 仅用于背景矩形——背景透明时无需整遍 EstTextWidth/
        // EstTextHeight（逐字形查找）。仅背景不透明时度量。
        double bgAlpha = bgColor.A;
        double estimatedWidth = 0.0;
        double estimatedHeight = 0.0;
        if (bgAlpha > 0.001) {
            estimatedWidth = this.EstTextWidth(text, MinTextPaddingX, fs, familyIdx, weight);
            estimatedHeight = this.EstTextHeight(text, MinTextPaddingY, fs, familyIdx);
            this.DrawRect(x, y, estimatedWidth, estimatedHeight, bgColor);
        }

        // pen 起点（DIP）：左 padding + 顶部 padding 后第一行 baseline 在 ascent 处
        double penX = x + MinTextPaddingX / 2.0;
        double baselineY = y + MinTextPaddingY / 2.0 + ascentDip;
        int len = text.Length;
        int i = 0;
        while (i < len) {
            int cp = 0;
            int n = this.Utf8DecodeAt(text, i, out cp);
            if (n == 0) { break; }
            i += n;

            if (cp == '\n') {
                penX = x + MinTextPaddingX / 2.0;
                baselineY += lineHeightDip;
                continue;
            }
            if (cp == '\r') { continue; }

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
            if (r < 0 || gw <= 0.0 || gh <= 0.0) {
                // 字形缺失/空白：pen 前进半角空格宽度（近似）
                penX += GlyphWidth * (fs / GlyphHeight);
                continue;
            }
            if (_uniformOffset + UniformSlotSize > UniformBufferSize) {
                _overflowDropped++;
                break;
            }

            // glyph quad：xoff/yoff/adv/gw/gh 均来自 1:1 物理像素光栅（size_px 域），
            // quad 原点对齐到整数像素避免子像素模糊，quad 尺寸 = 光栅尺寸（零缩放采样）。
            double quadX = Math.Floor(penX * _dpiScale + xoff + 0.5);
            double quadY = Math.Floor(baselineY * _dpiScale + yoff + 0.5);
            double quadW = gw;
            double quadH = gh;
            if (quadX < 8.0 && y > 60.0 && y < 220.0) {
                Console.WriteLine("[QUAD-DIAG] cp=" + cp + " penX=" + penX + " xoff=" + xoff + " quadX=" + quadX + " quadW=" + quadW);
            }
            if (quadW < 0.5) { quadW = 0.5; }
            if (quadH < 0.5) { quadH = 0.5; }

            // P3 阶段1：写入 CPU staging + 记录命令（帧末批量上传/重放）。
            wgpu_native.wgpu_batch_text_write(
                _staging, _uniformOffset,
                quadX, quadY, quadW, quadH,
                u0, v0, u1, v1,
                fr, fg, fb, fa,
                _surfaceWidth, _surfaceHeight);
            _cmdOffset.Add(_uniformOffset);
            _cmdPipeline.Add(1);  // 1 = text pipeline
            this.RecordCommandScissor();
            _uniformOffset += UniformSlotSize;

            // pen 前进 advance（DIP）：adv 为物理像素，转 DIP
            penX += adv / _dpiScale;
        }
    }

    // ============================================================
    // RFC 037 M1: ExecuteDrawList —— 批绘 DrawList IR（主路径）
    // ============================================================

    /// <summary>
    /// 批绘 DrawList IR（RFC 037）：遍历 DrawCommand，分发到对应 Draw* 方法。
    /// 须在 BeginFrame/EndFrame 之间调用。
    /// </summary>
    /// <returns>0 成功；-1 未初始化或未开始 RenderPass；-2 含不支持命令。</returns>
    public int ExecuteDrawList(DrawList list) {
        if (!_initialized || _pass == null || list == null) {
            return -1;
        }
        int n = list.Count;
        for (int i = 0; i < n; i++) {
            DrawCommand cmd = list.CommandAt(i);
            switch (cmd)
            {
                case DrawCommand.FillRect(r):
                {
                    this.DrawRect(r.X, r.Y, r.Width, r.Height, Color.Parse(r.FillColor));
                }
                case DrawCommand.DrawLine(l):
                {
                    this.DrawLine(l.X1, l.Y1, l.X2, l.Y2, Color.Parse(l.Color), l.Thickness);
                }
                case DrawCommand.DrawText(t):
                {
                    this.DrawText(t.Text, t.X, t.Y, t.FontSize,
                                 Color.Parse(t.Background), Color.Parse(t.Foreground));
                }
                case DrawCommand.DrawTexture(t):
                {
                    this.DrawTexture(t.TextureId, t.X, t.Y, t.Width, t.Height,
                                     t.SrcU0, t.SrcV0, t.SrcU1, t.SrcV1, t.Alpha);
                }
                default:
                {
                    return -2;
                }
            }
        }
        return 0;
    }

    /// <summary>
    /// 绘制线段——RFC 037 M1 占位实现：用细矩形近似（沿线段包围盒）。
    /// 真实顶点缓冲光栅化（set_vertex_buffer / draw_indexed）M2 落地。
    /// </summary>
    public void DrawLine(double x1, double y1, double x2, double y2,
                         Color color, double thickness) {
        if (!_initialized || _pass == null) {
            return;
        }
        double t = thickness;
        if (t < 1.0) {
            t = 1.0;
        }
        double minx;
        double maxx;
        double miny;
        double maxy;
        if (x1 < x2) { minx = x1; maxx = x2; } else { minx = x2; maxx = x1; }
        if (y1 < y2) { miny = y1; maxy = y2; } else { miny = y2; maxy = y1; }
        double bw = maxx - minx;
        double bh = maxy - miny;
        if (bw < t) { bw = t; }
        if (bh < t) { bh = t; }
        this.DrawRect(minx, miny, bw, bh, color);
    }

    /// <summary>
    /// 解析颜色字符串（"#RRGGBB" 6 位 或 "#AARRGGBB" 8 位——.NET/XAML AARRGGBB 约定）→ RGBA 0.0-1.0。
    /// 失败时默认黑色不透明。
    /// </summary>
    private double SrgbToLinear(double cs) {
        if (cs <= 0.04045) {
            return cs / 12.92;
        }
        return Math.Pow((cs + 0.055) / 1.055, 2.4);
    }

}
