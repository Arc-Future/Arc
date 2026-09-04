// RFC 037 §10 AI 原生 AL-P0: PngEncoder —— RGBA8 像素缓冲 → PNG 文件。
//
// 复用既有图像编码 ABI（rt_image_encode_png / rt_image_write_buffer / rt_image_free，
// crates/runtime-drawing/rt_image.c）——**禁双实现**：经 PngNative facade（纯 stub
// 类，builtin_facade + builtin_dispatch 双登记，见 PngNative.as）发射同一 C 调用，
// 不复制编码逻辑。Arc.Drawing.Bitmap 已提供公开 Save(path) 面，本类是渲染域
// （Arc.UI.Rendering）对像素缓冲直出 PNG 的轻量门面（Bitmap 内部缓冲不可达）。
//
// rgba 参数为 rt_image_alloc 形态的像素缓冲句柄（long）。
namespace Arc.UI.Rendering;

/// <summary>
/// RGBA8 像素缓冲 → PNG 文件编码（复用 rt_image_encode_png ABI；禁双实现）。
/// </summary>
public class PngEncoder {
    /// <summary>
    /// RGBA8 像素缓冲（rt_image_alloc 形态，long 句柄）→ PNG 文件。成功 true。
    /// 失败（参数错/编码失败/落盘失败）返回 false 并释放中间缓冲，不抛异常。
    /// </summary>
    public static bool Encode(string filePath, long rgba, int width, int height) {
        if (filePath == null || filePath == "" || rgba == 0 || width <= 0 || height <= 0) {
            return false;
        }
        long buf = 0;
        long len = 0;
        int rc = PngNative.EncodePng(rgba, width, height, out buf, out len);
        if (rc != 0 || buf == 0) {
            return false;
        }
        int ok = PngNative.WriteBuffer(filePath, buf, len);
        PngNative.Free(buf);
        return ok != 0;
    }
}
