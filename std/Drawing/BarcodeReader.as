// RFC 029 M3/M5: 1D barcode decode - BarcodeReader.Read(Bitmap).
//
// 设计（对齐 RFC 029 §1.4 ② + §3 M4/M5）：条码模块 = BarcodeWriter（生成）+
// BarcodeReader（解码）。
//   - 原生 1D 解码为主路径（EAN-13/Code39/Code128 · rt_barcode_1d_decode，
//     静态内置、开箱即用）；
//   - zxing 可选增强兜底（M5：zxing.ani load="auto" + ARC_ZXING_LIB），
//     用于照片/旋转/模糊等原生路径不适用的场景；
//   - 均失败 -> BarcodeNotFoundException（显式失败面，不静默 0/null）。
//   - 机制可能多轨，API 单轨：Read(Bitmap) 是唯一入口（RFC 029 §1.6）。
//
// 语言表面约束：禁位运算、禁 `new T[expr]` 动态尺寸（缓冲经 BarcodePixels 工具）。

namespace Arc.Drawing;

using Arc;

/// <summary>
/// 条码解码器（RFC 029 M4 + M5）。原生 1D 解码为主路径（开箱即用），
/// zxing 可选增强兜底（zxing.ani load="auto" + Native.IsAvailable 门闩降级）。
/// </summary>
public static class BarcodeReader {
    /// <summary>zxing 可选增强是否可用（`.ani` load="auto" 门闩：库经 ARC_ZXING_LIB
    /// 可加载 → true；否则 false）。原生 1D 解码不依赖它，始终开箱即用。</summary>
    public static bool IsZxingAvailable {
        get { return Native.IsAvailable("zxing"); }
    }

    /// <summary>Decode the first 1D barcode (EAN-13/Code39/Code128) from a bitmap
    /// and return its payload text. 原生解码优先（开箱即用），zxing 可选增强兜底。
    /// Throws <see cref="BarcodeNotFoundException"/> when nothing is found.</summary>
    public static string Read(Bitmap bm) {
        if (bm == null) {
            throw new ArgumentNullException("bm");
        }
        int w = bm.Width;
        int h = bm.Height;
        int textCap = 256;

        // 1. 原生 1D 解码（静态内置 · 开箱即用）：EAN-13 / Code39 / Code128。
        byte[] rgba = BarcodePixels.PackRgba(bm).ToArray();
        byte[] textArr = BarcodePixels.AllocText(textCap).ToArray();
        int rc = BarcodeNative.OneDDecode(rgba, w, h, textArr, textCap);
        if (rc == 0) {
            string t = BarcodePixels.ExtractText(textArr, textCap);
            if (t != null) {
                return t;
            }
        }

        // 2. zxing 通用解码（可选 .ani load="auto" + ARC_ZXING_LIB）。门闩未开 →
        //    跳过，不静默 stub。用于照片/旋转等原生路径不覆盖的场景。
        bool zxingAvail = Native.IsAvailable("zxing");
        if (zxingAvail) {
            List<byte> zrgba = BarcodePixels.PackRgba(bm);
            List<byte> ztextBuf = BarcodePixels.AllocText(textCap);
            int zrc = zxing.zxing_decode_c(zrgba, w, h, ztextBuf, textCap);
            if (zrc == 0) {
                string t = BarcodePixels.ExtractText(ztextBuf.ToArray(), textCap);
                if (t != null) {
                    return t;
                }
            }
        }

        // 3. 均失败 → 显式 BarcodeNotFoundException（不静默 0/null）。
        throw new BarcodeNotFoundException("no decodable barcode found");
    }
}