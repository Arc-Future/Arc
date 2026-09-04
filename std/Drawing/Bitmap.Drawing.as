// RFC 029 M6锛欱itmap 缁樺埗鍥惧厓鈥斺€擠rawFillRect / DrawLine / DrawText锛坧artial 鎵╁睍锛夈€?
//
// 璁捐锛堝榻?RFC 029 搂1.4 鈶?+ 搂3 M6锛夛細
//   - `public partial class Bitmap` 鎵╁睍锛氬～鍏呯煩褰?/ Bresenham 绾挎 / 鏂囨湰鍏夋爡鍖栥€?
//   - DrawText 閫?UTF-8 鐮佺偣锛堥瀛楄妭鍖洪棿鍒嗙被 + 涔樺姞缁勫悎锛?*绂佷綅杩愮畻**鈥斺€?
//     Arc 琛ㄨ揪寮忔枃娉曞皻鏈?parse BitAnd/绉讳綅锛涗笌 rt_font.c 鐨?UTF-8 瑙ｇ爜璇箟涓€鑷达級锛?
//     姣忕爜鐐规杩?= MeasureTextWidth(璇ュ簭鍒?銆?
//   - DrawGlyph锛歯ull 鏌ヨ瀛楀舰鍖呭洿鐩?鈫?List<byte>+ToArray 棰勫垎閰?alpha 缂撳啿
//     锛?*绂?`new T[expr]` 鍔ㄦ€佸昂瀵?*锛夆啋 濉厖 鈫?alpha 娣峰悎鍐欏叆鐩爣鍍忕礌銆?

namespace Arc.Drawing;

using Arc.Collections;

public partial class Bitmap {
    /// <summary>濉厖鐭╁舰 [x, x+w) 脳 [y, y+h)锛岃秴鐣屽儚绱犲拷鐣ャ€?/summary>
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

    /// <summary>Bresenham 绾挎 (x1,y1)鈫?x2,y2)锛岃秴鐣屽儚绱犲拷鐣ャ€?/summary>
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

    /// <summary>鎸?UTF-8 鐮佺偣搴忓垪缁樺埗鏂囨湰锛岃捣濮?(x, y) 涓哄熀绾垮乏渚ч《鐐广€?/summary>
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

    /// <summary>缁樺埗鍗曚釜瀛楀舰锛坅lpha 娣峰悎锛夈€傚瓧褰㈢己澶?/ 鍖呭洿鐩掍负绌烘椂闈欓粯璺宠繃銆?/summary>
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
