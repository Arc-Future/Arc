// RFC 029 M2: Arc.Drawing — AnimatedGif 动画位图。
//
// 包装 stb_image 一次性解码的 GIF 全帧连续 RGBA8 缓冲（frames*w*h*4 字节）
// 与每帧延时（毫秒）数组。帧缓冲不拷贝——<see cref="Frame"/> 经
// rt_image_gif_frame 定位第 i 帧起始指针（句柄），供上传到 GPU 纹理。
//
// 所有权：构造包装 decode 输出的原生缓冲；Dispose 经 rt_image_free 释放
// （decode 缓冲 + 延时数组均为 stb 分配，统一 free）。不可变读：Width/Height/
// FrameCount/DelayMs。内部构造（包内可见）——仅经 <see cref="ImageDecoder.DecodeGif"/>
// 创建，不对外暴露原生句柄。
namespace Arc.Drawing;

using Arc;
using Arc.IO;

/// <summary>GIF 动画位图——多帧 RGBA8 缓冲 + 每帧延时（毫秒）。IDisposable。</summary>
public class AnimatedGif : IDisposable {
    private long _handle;
    private long _delays;
    private bool _disposed;

    /// <summary>帧宽（像素）。</summary>
    public int Width { get; }
    /// <summary>帧高（像素）。</summary>
    public int Height { get; }
    /// <summary>总帧数。</summary>
    public int FrameCount { get; }

    /// <summary>包装 decode 输出的原生缓冲（供 ImageDecoder.DecodeGif；包内可见）。</summary>
    internal AnimatedGif(long handle, long delays, int width, int height, int frameCount) {
        _handle = handle;
        _delays = delays;
        this.Width = width;
        this.Height = height;
        this.FrameCount = frameCount;
    }

    /// <summary>第 index 帧延时（毫秒）。index 越界抛 ArgumentOutOfRangeException。</summary>
    public int DelayMs(int index) {
        this.EnsureAlive();
        if (index < 0 || index >= this.FrameCount) {
            throw new ArgumentOutOfRangeException("frame index out of range");
        }
        int ms = ImageNative.GifDelay(_delays, index);
        if (ms < 0) {
            ms = 0;
        }
        return ms;
    }

    /// <summary>第 index 帧像素缓冲句柄（RGBA8，width*height*4 字节；零拷贝）。
    /// 供离屏消费方（如 Arc.UI Image 组件）上传 GPU 纹理；句柄由本对象持有，
    /// 勿单独释放。index 越界抛 ArgumentOutOfRangeException。</summary>
    public long Frame(int index) {
        this.EnsureAlive();
        if (index < 0 || index >= this.FrameCount) {
            throw new ArgumentOutOfRangeException("frame index out of range");
        }
        return ImageNative.GifFrame(_handle, this.Width, this.Height, index);
    }

    public void Dispose() {
        if (_disposed) {
            return;
        }
        _disposed = true;
        if (_handle != 0) {
            ImageNative.Free(_handle);
            _handle = 0;
        }
        if (_delays != 0) {
            ImageNative.Free(_delays);
            _delays = 0;
        }
    }

    private void EnsureAlive() {
        if (_disposed) {
            throw new ObjectDisposedException("AnimatedGif");
        }
    }
}
