namespace Arc.Drawing;

/// <summary>
/// 二维码编码 C ABI 门面（std/Drawing 内部使用）——codegen 拦截
/// `QrCodeNative.*` 静态调用并发射 `rt_qrcode_*`（crates/runtime-drawing/rt_qrcode.c）。
///
/// modules 形态（对齐 rt_qrcode.c）：调用方预分配 ≥ 3918 字节；成功时
/// modules[0] = 模块边长（21..177），模块 (x,y) 存于
/// modules[1 + (y*size+x)/8] 的第 ((y*size+x) % 8) 位（LSB-first）。
///
/// 返回约定：成功 0 / 失败非零（text 过长 / ecc 越界 / mask 越界等）。
/// 本类**内部实现细节**（`internal`）——仅经 <see cref="QrCodeWriter"/> 使用，不对类库使用者暴露。
/// </summary>
internal class QrCodeNative {
    /// <summary>文本 → bit-packed 模块矩阵。成功 0 / 失败非零。</summary>
    [Builtin(ABI = "rt_qrcode_encode")]
    public static int Encode(string text, int ecc, int mask, byte[] modules, out int size) { return -1; }
}
