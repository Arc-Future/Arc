namespace Arc.Drawing;

using Arc;
using Arc.IO;

/// <summary>
/// 位图（RGBA8 像素面）。M1 像素缓冲形态 = **NativePtr 句柄**：内部持
/// <c>long _handle</c>（原生 RGBA8 缓冲指针的整数形态），由
/// <see cref="ImageNative"/>（rt_image_* ABI）操作；<see cref="Dispose"/>
/// 触发 <c>rt_image_free</c>。GetPixel/SetPixel 越界抛 ArgumentOutOfRangeException。
/// </summary>
public partial class Bitmap : IDisposable {
    private long _handle;
    private bool _disposed;

    public int Width { get; }
    public int Height { get; }

    /// <summary>M1 固定 Rgba32。</summary>
    public PixelFormat PixelFormat { get; } = PixelFormat.Rgba32;

    /// <summary>新建空白位图（零初始化像素）。尺寸非法或分配失败抛异常。</summary>
    public Bitmap(int width, int height) {
        if (width <= 0 || height <= 0) {
            throw new ArgumentException("Bitmap width and height must be positive");
        }
        long handle = ImageNative.Alloc(width, height);
        if (handle == 0) {
            throw new InvalidOperationException("Failed to allocate bitmap pixel buffer");
        }
        _handle = handle;
        this.Width = width;
        this.Height = height;
    }

    /// <summary>包装 decode 输出的原生缓冲（供 ImageDecoder.Decode；包内可见）。</summary>
    internal Bitmap(long handle, int width, int height) {
        _handle = handle;
        this.Width = width;
        this.Height = height;
    }

    /// <summary>
    /// 原生 RGBA8 像素缓冲句柄（width*height*4 字节；零拷贝）。供离屏消费方
    /// （如 Arc.UI Image 组件）上传 GPU 纹理；句柄由本对象持有，勿单独释放。
    /// 释放经 <see cref="Dispose"/>。已释放调用抛 ObjectDisposedException。
    /// </summary>
    public long GetPixels() {
        this.EnsureAlive();
        return _handle;
    }

    /// <summary>取像素。坐标越界抛 ArgumentOutOfRangeException。</summary>
    public RgbColor GetPixel(int x, int y) {
        this.EnsureAlive();
        long argb = ImageNative.GetPixel(_handle, this.Width, this.Height, x, y);
        if (argb < 0) {
            throw new ArgumentOutOfRangeException("pixel coordinates out of range");
        }
        return new RgbColor(
            (byte)(argb / (long)16777216),
            (byte)((argb / (long)65536) % (long)256),
            (byte)((argb / (long)256) % (long)256),
            (byte)(argb % (long)256));
    }

    /// <summary>写像素。坐标越界抛 ArgumentOutOfRangeException。</summary>
    public void SetPixel(int x, int y, RgbColor color) {
        this.EnsureAlive();
        long argb = (long)color.A * (long)16777216 + (long)color.R * (long)65536
            + (long)color.G * (long)256 + (long)color.B;
        int rc = ImageNative.SetPixel(_handle, this.Width, this.Height, x, y, argb);
        if (rc == 0) {
            throw new ArgumentOutOfRangeException("pixel coordinates out of range");
        }
    }

    /// <summary>
    /// 按文件扩展名保存：.png → PNG；.jpg/.jpeg → JPEG。其余格式
    /// （含 ImageFormat.Bmp/Tga）抛 NotSupportedException（M1 未暴露编码 ABI）。
    /// </summary>
    public void Save(string path) {
        this.EnsureAlive();
        string ext = Path.GetExtension(path).ToLower();
        if (ext == ".png") {
            this.SavePng(path);
        } else if (ext == ".jpg" || ext == ".jpeg") {
            this.SaveJpg(path);
        } else {
            throw new NotSupportedException("Unsupported image format: " + ext);
        }
    }

    /// <summary>Stream 保存重载（Save(Stream, ImageFormat)）为如实登记的后置：
    /// 显式格式编码的 Stream 面待 Stream 消费点（RFC 016）就绪后升 Stable
    /// （RFC 029 §1.4）；当前编码缓冲直落盘经 rt_image_write_buffer。</summary>
    // public void Save(Stream stream, ImageFormat format) { }

    public void Dispose() {
        if (_disposed) {
            return;
        }
        _disposed = true;
        if (_handle != 0) {
            ImageNative.Free(_handle);
            _handle = 0;
        }
    }

    private void SavePng(string path) {
        long buf = 0;
        long len = 0;
        int rc = ImageNative.EncodePng(_handle, this.Width, this.Height, out buf, out len);
        if (rc != 0 || buf == 0) {
            throw new IOException("PNG encoding failed");
        }
        int ok = ImageNative.WriteBuffer(path, buf, len);
        ImageNative.Free(buf);
        if (ok == 0) {
            throw new IOException("Failed to write file: " + path);
        }
    }

    private void SaveJpg(string path) {
        long buf = 0;
        long len = 0;
        int rc = ImageNative.EncodeJpg(_handle, this.Width, this.Height, 90, out buf, out len);
        if (rc != 0 || buf == 0) {
            throw new IOException("JPEG encoding failed");
        }
        int ok = ImageNative.WriteBuffer(path, buf, len);
        ImageNative.Free(buf);
        if (ok == 0) {
            throw new IOException("Failed to write file: " + path);
        }
    }

    private void EnsureAlive() {
        if (_disposed) {
            throw new ObjectDisposedException("Bitmap");
        }
    }
}
