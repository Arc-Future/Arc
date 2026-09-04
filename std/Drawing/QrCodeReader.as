// RFC 029 M4：QR 二维码解码——quirc（vendored C）经 BarcodeNative.QuircDecode 直射。
//
// 设计（对齐 RFC 029 §1.4 ① + §3 M4）：QR 模块 = QrCodeWriter（生成）+ QrCodeReader（解码）。
// 开箱即用且功能齐备：原生 quirc 静态内置，不依赖可选 zxing。
//
// 语言表面约束：禁位运算、禁 `new T[expr]` 动态尺寸（缓冲经 BarcodePixels 工具）。

namespace Arc.Drawing;

/// <summary>
/// 二维码解码器（RFC 029 M4）。原生 quirc 路径：RGBA8 像素 → 灰度 → 解码首个 QR →
/// NUL 终止文本。开箱即用（不依赖可选 zxing）。
/// </summary>
public static class QrCodeReader {
    /// <summary>解码位图中首个 QR，返回载荷文本。未找到抛
    /// <see cref="BarcodeNotFoundException"/>（显式失败面，禁静默 0/null）。</summary>
    public static string Read(Bitmap bm) {
        if (bm == null) {
            throw new ArgumentNullException("bm");
        }
        int w = bm.Width;
        int h = bm.Height;
        byte[] rgba = BarcodePixels.PackRgba(bm).ToArray();
        int textCap = 256;
        byte[] textArr = BarcodePixels.AllocText(textCap).ToArray();
        int rc = BarcodeNative.QuircDecode(rgba, w, h, textArr, textCap);
        if (rc == 0) {
            string t = BarcodePixels.ExtractText(textArr, textCap);
            if (t != null) {
                return t;
            }
        }
        throw new BarcodeNotFoundException("no decodable QR found");
    }
}