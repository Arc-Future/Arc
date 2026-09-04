// RFC 029 M4/M5：BarcodeReader / QrCodeReader 共享的像素打包与文本缓冲工具。
//
// 设计（对齐 RFC 029 §1.4 ① + §3 M4/M5）：两个解码 reader 都需把 Bitmap RGBA8
// 像素面打包为 (R,G,B,A) 顺序缓冲、预分配 NUL 终止文本缓冲、并从 NUL 终止缓冲
// 提取文本。抽为包内共享 **internal** 工具，避免两处重复；不暴露给类库使用者。
//
// 语言表面约束：禁位运算、禁 `new T[expr]` 动态尺寸——缓冲用 List<byte> + Add + ToArray()。

namespace Arc.Drawing;

using Arc.Collections;
using Arc.Text;

/// <summary>条码 / QR 解码共享的 RGBA 打包与 NUL 终止文本提取（**内部实现细节**，
/// 仅经 <see cref="BarcodeReader"/> / <see cref="QrCodeReader"/> 使用）。</summary>
internal static class BarcodePixels {
    /// <summary>把 Bitmap 像素面打包为 RGBA8 的 List&lt;byte&gt;（R,G,B,A 顺序，
    /// 对齐 rt_barcode_* / zxing ImageFormat::RGBA 的通道解析）。</summary>
    public static List<byte> PackRgba(Bitmap bm) {
        int w = bm.Width;
        int h = bm.Height;
        List<byte> rgba = new List<byte>();
        int y = 0;
        while (y < h) {
            int x = 0;
            while (x < w) {
                RgbColor c = bm.GetPixel(x, y);
                rgba.Add(c.R);
                rgba.Add(c.G);
                rgba.Add(c.B);
                rgba.Add(c.A);
                x = x + 1;
            }
            y = y + 1;
        }
        return rgba;
    }

    /// <summary>预分配 NUL 终止文本缓冲（<paramref name="cap"/> 字节，全 0）。</summary>
    public static List<byte> AllocText(int cap) {
        List<byte> buf = new List<byte>();
        int i = 0;
        while (i < cap) {
            buf.Add((byte)0);
            i = i + 1;
        }
        return buf;
    }

    /// <summary>从 NUL 终止缓冲提取文本；无内容返回 null（上层据此走增强路径）。</summary>
    public static string ExtractText(byte[] buf, int cap) {
        List<byte> payload = new List<byte>();
        int i = 0;
        while (i < cap) {
            if (buf[i] == (byte)0) {
                break;
            }
            payload.Add(buf[i]);
            i = i + 1;
        }
        if (payload.Count == 0) {
            return null;
        }
        byte[] payloadBytes = payload.ToArray();
        return Encoding.GetString(payloadBytes);
    }
}