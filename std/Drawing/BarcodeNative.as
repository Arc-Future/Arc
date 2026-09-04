namespace Arc.Drawing;

/// <summary>
/// 条形码解码 C ABI 门面（std/Drawing 内部使用）——codegen 拦截
/// `BarcodeNative.*` 静态调用并发射 `rt_barcode_*`
/// （crates/runtime-drawing/rt_barcode.c，quirc 单 TU 合并）。
///
/// 输入：RGBA8 像素缓冲（对齐 Bitmap Rgba32 语义）；输出：NUL 终止文本
/// 缓冲（textCap 字节，最大载荷 textCap-1）。
///
/// 返回约定：成功 0 / 失败非零（无码 / 解码失败 / 非法输入）。
/// 本类**内部实现细节**（`internal`）——仅经 <see cref="BarcodeReader"/> / <see cref="QrCodeReader"/> 使用，不对类库使用者暴露。
/// </summary>
internal class BarcodeNative {
    /// <summary>RGBA8 像素 → quirc 解码首个 QR → NUL 终止文本。成功 0 / 失败非零。</summary>
    [Builtin(ABI = "rt_barcode_quirc_decode")]
    public static int QuircDecode(byte[] rgba, int w, int h, byte[] textOut, int textCap) { return -1; }

    /// <summary>RGBA8 像素 → 原生 1D 解码（EAN-13/Code39/Code128）→ NUL 终止文本。
    /// 成功 0 / 失败非零（无码 / 解码失败 / 非法输入）。</summary>
    [Builtin(ABI = "rt_barcode_1d_decode")]
    public static int OneDDecode(byte[] rgba, int w, int h, byte[] textOut, int textCap) { return -1; }
}
