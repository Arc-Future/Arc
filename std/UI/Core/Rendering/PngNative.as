// RFC 037 §10 AI 原生 AL-P0: PngNative —— rt_image_* 编码 ABI facade（纯 stub 类）。
//
// builtin_dispatch 按 "PngNative." 类名前缀拦截 rt_image_encode_png / write_buffer /
// free（emit_call.rs try_emit_image_native_static，复用 ImageNative 分派）；typeck
// is_builtin_facade 跳过 stub 方法体。facade 类必须为**纯 stub**（PngEncoder 含真实
// 逻辑，不能入清单）。语义为复用既有 C ABI（禁双实现：无编码逻辑，仅 ABI 声明）。
namespace Arc.UI.Rendering;

/// <summary>
/// rt_image_* 编码 ABI facade（PngEncoder 专用；复用既有 C 实现，禁双实现）。
/// </summary>
public class PngNative {
    /// <summary>RGBA8 缓冲 → PNG 内存缓冲。成功 0 / 失败非零。</summary>
    [Builtin(ABI = "rt_image_encode_png")]
    public static int EncodePng(long rgba, int w, int h, out long buf, out long len) { return -1; }

    /// <summary>编码缓冲原样写入文件路径。成功 1 / 失败 0。</summary>
    [Builtin(ABI = "rt_image_write_buffer")]
    public static int WriteBuffer(string path, long buf, long len) { return 0; }

    /// <summary>释放编码缓冲（rt_image_free = free）。</summary>
    [Builtin(ABI = "rt_image_free")]
    public static void Free(long p) { }
}
