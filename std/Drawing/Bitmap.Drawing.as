// RFC 029 M6：Bitmap 绘制图元——DrawFillRect / DrawLine / DrawText（partial 扩展）。
//
// 设计（对齐 RFC 029 §1.4 ④ + §3 M6）：
//   - `public partial class Bitmap` 扩展：填充矩形 / Bresenham 线段 / 文本光栅化。
//   - DrawText 逐 UTF-8 码点（首字节区间分类 + 乘加组合，**禁位运算**——
//     Arc 表达式文法尚未 parse BitAnd/移位；与 rt_font.c 的 UTF-8 解码语义一致），
//     每码点步进 = MeasureTextWidth(该序列)。
//   - DrawGlyph：null 查询字形包围盒 → List<byte>+ToArray 预分配 alpha 缓冲
//     （**禁 `new T[expr]` 动态尺寸**）→ 填充 → alpha 混合写入目标像素。

namespace Arc.Drawing;

using Arc.Collections;

public partial class Bitmap {
    /// <summary>填充矩形 [x, x+w) × [y, y+h)，超界像素忽略。</summary>
    public void DrawFillRect(int x, int y, int w, int h, RgbColor color) {
        if (w <= 0 || h <= 0) {
            return;
        }
        int xEnd = x + w;
        int yEnd = y + h;
        int cx = x;
        while (cx < xEnd) {
            int cy = y;
            while (cy < yEnd) {
                if (cx >= 0 && cx < this.Width && cy >= 0 && cy < this.Height) {
                    this.SetPixel(cx, cy, color);
                }
                cy = cy + 1;
            }
            cx = cx + 1;
        }
    }

    /// <summary>Bresenham 线段 (x1,y1)→(x2,y2)，超界像素忽略。</summary>
    public void DrawLine(int x1, int y1, int x2, int y2, RgbColor color) {
        int dx = x2 - x1;
        int dy = y2 - y1;
        int sx = 1;
        if (dx < 0) { sx = -1; }
        int sy = 1;
        if (dy < 0) { sy = -1; }
        int ax = dx;
        if (ax < 0) { ax = -ax; }
        int ay = dy;
        if (ay < 0) { ay = -ay; }
        int err = ax - ay;
        int x = x1;
        int y = y1;
        while (true) {
            if (x >= 0 && x < this.Width && y >= 0 && y < this.Height) {
                this.SetPixel(x, y, color);
            }
            if (x == x2 && y == y2) {
                break;
            }
            int e2 = err * 2;
            if (e2 > -ay) {
                err = err - ay;
                x = x + sx;
            }
            if (e2 < ax) {
                err = err + ax;
                y = y + sy;
            }
        }
    }

    /// <summary>按 UTF-8 码点序列绘制文本，起始 (x, y) 为基线左侧顶点。</summary>
    public void DrawText(Font font, string text, int x, int y, RgbColor color) {
        if (font == null || text == null) {
            return;
        }
        int i = 0;
        int n = text.Length;
        while (i < n) {
            int cp = 0;
            int seqLen = 1;
            int b0 = (int)text[i];
            if (b0 < 128) {
                cp = b0;
            } else if (b0 >= 194 && b0 <= 223 && i + 1 < n) {
                int b1 = (int)text[i + 1];
                if (b1 >= 128 && b1 <= 191) {
                    cp = (b0 - 192) * 64 + (b1 - 128);
                    seqLen = 2;
                } else {
                    cp = b0;
                }
            } else if (b0 >= 224 && b0 <= 239 && i + 2 < n) {
                int b1 = (int)text[i + 1];
                int b2 = (int)text[i + 2];
                if (b1 >= 128 && b1 <= 191 && b2 >= 128 && b2 <= 191) {
                    cp = ((b0 - 224) * 64 + (b1 - 128)) * 64 + (b2 - 128);
                    seqLen = 3;
                } else {
                    cp = b0;
                }
            } else if (b0 >= 240 && b0 <= 244 && i + 3 < n) {
                int b1 = (int)text[i + 1];
                int b2 = (int)text[i + 2];
                int b3 = (int)text[i + 3];
                if (b1 >= 128 && b1 <= 191 && b2 >= 128 && b2 <= 191 && b3 >= 128 && b3 <= 191) {
                    cp = (((b0 - 240) * 64 + (b1 - 128)) * 64 + (b2 - 128)) * 64 + (b3 - 128);
                    seqLen = 4;
                } else {
                    cp = b0;
                }
            } else {
                cp = b0;
            }
            this.DrawGlyph(font, cp, x, y, color);
            x = x + (int)font.MeasureTextWidth(text.Substring(i, seqLen));
            i = i + seqLen;
        }
    }

    /// <summary>绘制单个字形（alpha 混合）。字形缺失 / 包围盒为空时静默跳过。</summary>
    private void DrawGlyph(Font font, int codepoint, int x, int y, RgbColor color) {
        int w = 0;
        int h = 0;
        float xoff = 0.0;
        float yoff = 0.0;
        int rc = font.Glyph(codepoint, null, out w, out h, out xoff, out yoff);
        if (rc != 0 || w <= 0 || h <= 0) {
            return;
        }
        List<byte> alphaBuf = new List<byte>();
        int total = w * h;
        int k = 0;
        while (k < total) {
            alphaBuf.Add((byte)0);
            k = k + 1;
        }
        byte[] alpha = alphaBuf.ToArray();
        rc = font.Glyph(codepoint, alpha, out w, out h, out xoff, out yoff);
        if (rc != 0) {
            return;
        }
        int ox = (int)xoff;
        int oy = (int)yoff;
        int py = 0;
        while (py < h) {
            int px = 0;
            while (px < w) {
                int a = (int)alpha[py * w + px];
                if (a > 0) {
                    int gx = x + ox + px;
                    int gy = y + oy + py;
                    if (gx >= 0 && gx < this.Width && gy >= 0 && gy < this.Height) {
                        RgbColor dst = this.GetPixel(gx, gy);
                        int inv = 255 - a;
                        byte r = (byte)(((int)dst.R * inv + (int)color.R * a) / 255);
                        byte g = (byte)(((int)dst.G * inv + (int)color.G * a) / 255);
                        byte bl = (byte)(((int)dst.B * inv + (int)color.B * a) / 255);
                        this.SetPixel(gx, gy, new RgbColor((byte)255, r, g, bl));
                    }
                }
                px = px + 1;
            }
            py = py + 1;
        }
    }
}
