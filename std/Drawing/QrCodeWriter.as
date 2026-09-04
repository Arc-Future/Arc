// RFC 029 M2：QR 二维码生成——qrcodegen（vendored C）经 QrCodeNative 直射。
//
// 设计（对齐 RFC 029 §1.4 ① + §3 M2）：QR 模块 = QrCodeWriter（生成）+ QrCodeReader（解码）。
//   - 公有 API：Encode(text[, ecc[, mask]]) → Bitmap（模块 scale 4 · quiet zone 4 模块）。
//   - ECC 判别值直射 qrcodegen（L=0/M=1/Q=2/H=3）；mask -1 = 自动。
//   - 确定性：同 (text, ecc, mask) → 同一模块矩阵（qrcodegen 纯函数）。
//
// 语言表面约束（各里程碑已确认）：
//   - 禁位运算：模块 bit 读取用除模（LSB-first bit-packed 展开为乘 2 查表）。
//   - 禁 `new T[expr]` 动态尺寸：模块缓冲用 List<byte> + Add + ToArray()。

namespace Arc.Drawing;

using Arc.Collections;

/// <summary>
/// 二维码生成器（RFC 029 M2）。编码为 Bitmap：黑模块白背景，模块 4px、
/// 四周 4 模块 quiet zone。QR 模块解码侧见 <see cref="QrCodeReader"/>。
/// </summary>
public static class QrCodeWriter {
    /// <summary>编码文本 → Bitmap（默认 ECC M · 掩码自动）。文本过长抛 ArgumentException。</summary>
    public static Bitmap Encode(string text) {
        return QrCodeWriter.Encode(text, QrCodeErrorCorrection.M, -1);
    }

    /// <summary>编码文本 → Bitmap（显式 ECC · 掩码自动）。</summary>
    public static Bitmap Encode(string text, QrCodeErrorCorrection ecc) {
        return QrCodeWriter.Encode(text, ecc, -1);
    }

    /// <summary>编码文本 → Bitmap（显式 ECC · 掩码 0..7，-1 = 自动）。</summary>
    public static Bitmap Encode(string text, QrCodeErrorCorrection ecc, int mask) {
        if (text == null || text == "") {
            throw new ArgumentException("QrCodeWriter.Encode: 输入为空");
        }
        byte[] modules = QrCodeWriter._AllocModulesBuffer();
        int size = 0;
        int rc = QrCodeNative.Encode(text, (int)ecc, mask, modules, out size);
        if (rc != 0 || size < 21) {
            throw new ArgumentException("QrCodeWriter.Encode: 文本过长或编码失败");
        }

        int scale = 4;
        int quiet = 4;
        int px = (size + quiet * 2) * scale;
        Bitmap bm = new Bitmap(px, px);
        RgbColor black = RgbColor.FromArgb((byte)0, (byte)0, (byte)0);
        RgbColor white = RgbColor.FromArgb((byte)255, (byte)255, (byte)255);

        int x = 0;
        while (x < px) {
            int y = 0;
            while (y < px) {
                bm.SetPixel(x, y, white);
                y = y + 1;
            }
            x = x + 1;
        }

        int my = 0;
        while (my < size) {
            int mx = 0;
            while (mx < size) {
                if (QrCodeWriter._IsDark(modules, size, mx, my)) {
                    int sx = (quiet + mx) * scale;
                    int sy = (quiet + my) * scale;
                    int q = 0;
                    while (q < scale) {
                        int r = 0;
                        while (r < scale) {
                            bm.SetPixel(sx + q, sy + r, black);
                            r = r + 1;
                        }
                        q = q + 1;
                    }
                }
                mx = mx + 1;
            }
            my = my + 1;
        }
        return bm;
    }

    /// <summary>模块 (x,y) 是否暗模块（LSB-first bit-packed，除模读取）。</summary>
    private static bool _IsDark(byte[] modules, int size, int x, int y) {
        int idx = y * size + x;
        int byteIdx = 1 + idx / 8;
        int bitIdx = idx % 8;
        int v = (int)modules[byteIdx] / QrCodeWriter._Pow2(bitIdx) % 2;
        return v == 1;
    }

    private static int _Pow2(int n) {
        int r = 1;
        int i = 0;
        while (i < n) {
            r = r * 2;
            i = i + 1;
        }
        return r;
    }

    /// <summary>预分配 qrcodegen 模块缓冲（qrcodegen_BUFFER_LEN_MAX = 3918）。</summary>
    private static byte[] _AllocModulesBuffer() {
        List<byte> buf = new List<byte>();
        int i = 0;
        while (i < 3918) {
            buf.Add((byte)0);
            i = i + 1;
        }
        return buf.ToArray();
    }
}
