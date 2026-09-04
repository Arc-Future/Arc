namespace Arc.Drawing;

/// <summary>
/// 图像编解码 C ABI 门面（std/Drawing 内部使用）——codegen 拦截
/// `ImageNative.*` 静态调用并发射 `rt_image_*`（crates/runtime-drawing/rt_image.c）。
///
/// 像素缓冲形态 = **NativePtr 句柄（long）**：Bitmap 持有原生 RGBA8 缓冲指针的
/// 整数形态；decode/encode 输出为 stbi/malloc 原生缓冲，释放经 <c>rt_image_free</c>。
///
/// 返回约定：成功 0 / 失败非零。`out long rgba` 输出缓冲句柄；`out int w/h` 尺寸；
/// `Encode*` 的 `out long buf` 为编码字节缓冲句柄、`out long len` 为其长度。
/// 本类**内部实现细节**（`internal`）——仅经 <see cref="Bitmap"/> / <see cref="Image"/> 使用，
/// 不对类库使用者暴露。
/// </summary>
internal class ImageNative {
    /// <summary>分配 w*h*4 字节零初始化 RGBA8 缓冲。失败返回 0。</summary>
    [Builtin(ABI = "rt_image_alloc")]
    public static long Alloc(int w, int h) { return 0; }

    /// <summary>解码内存字节 → RGBA8 缓冲。成功 0 / 失败非零。</summary>
    [Builtin(ABI = "rt_image_decode")]
    public static int Decode(byte[] data, out long rgba, out int w, out int h) { return -1; }

    /// <summary>解码文件路径 → RGBA8 缓冲。成功 0 / 失败非零。</summary>
    [Builtin(ABI = "rt_image_decode_file")]
    public static int DecodeFile(string path, out long rgba, out int w, out int h) { return -1; }

    /// <summary>RGBA8 缓冲 → PNG 内存缓冲。成功 0 / 失败非零。</summary>
    [Builtin(ABI = "rt_image_encode_png")]
    public static int EncodePng(long rgba, int w, int h, out long buf, out long len) { return -1; }

    /// <summary>RGBA8 缓冲 → JPEG 内存缓冲（quality 1..100）。成功 0 / 失败非零。</summary>
    [Builtin(ABI = "rt_image_encode_jpg")]
    public static int EncodeJpg(long rgba, int w, int h, int quality, out long buf, out long len) { return -1; }

    /// <summary>取像素 → 打包 ARGB（0x00AARRGGBB）。越界返回 -1。</summary>
    [Builtin(ABI = "rt_image_get_pixel")]
    public static long GetPixel(long rgba, int w, int h, int x, int y) { return -1; }

    /// <summary>写像素（打包 ARGB）。成功 1 / 越界 0。</summary>
    [Builtin(ABI = "rt_image_set_pixel")]
    public static int SetPixel(long rgba, int w, int h, int x, int y, long argb) { return 0; }

    /// <summary>实心矩形填充（打包 ARGB；与画布求交裁剪）。有写入 1 / 无效 0。
    /// std P2 批渲染专用——替代逐像素 SetPixel 循环。</summary>
    [Builtin(ABI = "rt_image_fill_rect")]
    public static int FillRect(long rgba, int w, int h, int x, int y, int rw, int rh, long argb) { return 0; }

    /// <summary>把编码缓冲原样写入文件路径。成功 1 / 失败 0。</summary>
    [Builtin(ABI = "rt_image_write_buffer")]
    public static int WriteBuffer(string path, long buf, long len) { return 0; }

    /// <summary>释放 decode/encode 输出的原生缓冲（rt_image_free = free）。</summary>
    [Builtin(ABI = "rt_image_free")]
    public static void Free(long p) { }

    // ---- RFC 029 M2：GIF 多帧解码 + SVG 光栅化（Image 多格式扩展）----

    /// <summary>解码 GIF → 全部帧连续 RGBA8 缓冲（frames*w*h*4）+ 每帧延时（毫秒）数组。
    /// 两个缓冲句柄均经 <see cref="Free"/> 释放。成功 0 / 失败非零。</summary>
    [Builtin(ABI = "rt_image_decode_gif")]
    public static int DecodeGif(byte[] data, out long rgba, out int w, out int h, out int frameCount, out long delays) { return -1; }

    /// <summary>定位 GIF 帧 i 的起始指针（无拷贝；越界返回 0）。</summary>
    [Builtin(ABI = "rt_image_gif_frame")]
    public static long GifFrame(long rgba, int w, int h, int frameIndex) { return 0; }

    /// <summary>读取 GIF 帧 i 的延时（毫秒；越界返回 -1）。</summary>
    [Builtin(ABI = "rt_image_gif_delay")]
    public static int GifDelay(long delays, int frameIndex) { return -1; }

    /// <summary>解码 SVG → 直通 RGBA8 缓冲（scale 为光栅化缩放，&lt;=0 按 1.0）。
    /// 缓冲句柄经 <see cref="Free"/> 释放。成功 0 / 失败非零。</summary>
    [Builtin(ABI = "rt_image_decode_svg")]
    public static int DecodeSvg(byte[] data, float scale, out long rgba, out int w, out int h) { return -1; }
}
