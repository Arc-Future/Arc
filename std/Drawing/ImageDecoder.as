namespace Arc.Drawing;

using Arc;
using Arc.IO;

/// <summary>
/// 图像解码门面（ImageDecoder）。解码产物为持原生缓冲的 <see cref="Bitmap"/>
/// （IDisposable，Dispose 释放原生像素缓冲）。GIF 多帧解码产物为
/// <see cref="AnimatedGif"/>（含每帧延时）。解码失败抛 IOException。
///
/// **命名说明（RFC 029 M1.1）**：本门面不叫 `Image` 而与 UI 组件
/// `Arc.UI.Components.Image` 同名——注册表以短名为键，两库导出同名类型会在
/// 消费者侧互相覆盖（external 注册「本地优先跳过」），导致 `Arc.Drawing.Image`
/// 无法被解析。故以职责命名 `ImageDecoder`，消除同名冲突（见 RFC 029 冲突说明）。
/// </summary>
public static class ImageDecoder {
    /// <summary>解码图像文件 → Bitmap。文件不存在或数据损坏抛 IOException。</summary>
    public static Bitmap Decode(string path) {
        long rgba = 0;
        int w = 0;
        int h = 0;
        int rc = ImageNative.DecodeFile(path, out rgba, out w, out h);
        if (rc != 0 || rgba == 0) {
            throw new IOException("Failed to decode image: " + path);
        }
        return new Bitmap(rgba, w, h);
    }

    /// <summary>解码图像字节 → Bitmap。空/损坏数据抛 IOException。</summary>
    public static Bitmap Decode(byte[] data) {
        if (data == null) {
            throw new ArgumentNullException("data");
        }
        long rgba = 0;
        int w = 0;
        int h = 0;
        int rc = ImageNative.Decode(data, out rgba, out w, out h);
        if (rc != 0 || rgba == 0) {
            throw new IOException("Failed to decode image data");
        }
        return new Bitmap(rgba, w, h);
    }

    // ---- RFC 029 M2：GIF 多帧 + SVG 光栅化（Image 多格式扩展）----

    /// <summary>解码 GIF 文件 → <see cref="AnimatedGif"/>（多帧 + 每帧延时）。
    /// 文件缺失/损坏抛 IOException。</summary>
    public static AnimatedGif DecodeGif(string path) {
        byte[] data = File.ReadAllBytes(path);
        if (data == null || data.Length == 0) {
            throw new IOException("Failed to read image: " + path);
        }
        return ImageDecoder.DecodeGif(data);
    }

    /// <summary>解码 GIF 字节 → <see cref="AnimatedGif"/>（多帧 + 每帧延时）。
    /// 空/损坏数据抛 IOException。</summary>
    public static AnimatedGif DecodeGif(byte[] data) {
        if (data == null) {
            throw new ArgumentNullException("data");
        }
        long rgba = 0;
        int w = 0;
        int h = 0;
        int frameCount = 0;
        long delays = 0;
        int rc = ImageNative.DecodeGif(data, out rgba, out w, out h, out frameCount, out delays);
        if (rc != 0 || rgba == 0 || frameCount <= 0) {
            if (rgba != 0) { ImageNative.Free(rgba); }
            if (delays != 0) { ImageNative.Free(delays); }
            throw new IOException("Failed to decode GIF image data");
        }
        return new AnimatedGif(rgba, delays, w, h, frameCount);
    }

    /// <summary>解码 SVG 文件 → <see cref="Bitmap"/>（scale 为光栅化缩放，&lt;=0 按 1.0）。
    /// 文件缺失/损坏抛 IOException。</summary>
    public static Bitmap DecodeSvg(string path, float scale) {
        byte[] data = File.ReadAllBytes(path);
        if (data == null || data.Length == 0) {
            throw new IOException("Failed to read image: " + path);
        }
        return ImageDecoder.DecodeSvg(data, scale);
    }

    /// <summary>解码 SVG 字节 → <see cref="Bitmap"/>（scale 为光栅化缩放，&lt;=0 按 1.0）。
    /// 空/损坏数据抛 IOException。</summary>
    public static Bitmap DecodeSvg(byte[] data, float scale) {
        if (data == null) {
            throw new ArgumentNullException("data");
        }
        long rgba = 0;
        int w = 0;
        int h = 0;
        int rc = ImageNative.DecodeSvg(data, scale, out rgba, out w, out h);
        if (rc != 0 || rgba == 0) {
            throw new IOException("Failed to decode SVG data");
        }
        return new Bitmap(rgba, w, h);
    }

    // ---- 格式探测（Image 组件 Source 解码路由用）----

    /// <summary>按魔数判断字节是否为 GIF：GIF8 前缀（0x47 0x49 0x46 0x38）。</summary>
    public static bool IsGif(byte[] data) {
        if (data == null || data.Length < 4) {
            return false;
        }
        return data[0] == (byte)71 && data[1] == (byte)73
            && data[2] == (byte)70 && data[3] == (byte)56;
    }

    /// <summary>按魔数判断字节是否为 SVG：&lt;svg 或 &lt;?xml 或 &lt;!DOCTYPE svg 前导
    /// （ASCII 不区分大小写；空/短数据返回 false）。</summary>
    public static bool IsSvg(byte[] data) {
        if (data == null || data.Length < 5) {
            return false;
        }
        int i = 0;
        // 跳过 UTF-8 BOM。
        if (data.Length >= 3 && data[0] == (byte)239 && data[1] == (byte)187 && data[2] == (byte)191) {
            i = 3;
        }
        if (data.Length - i < 5) {
            return false;
        }
        if (data[i] != (byte)60) {  // '<'
            return false;
        }
        int b1 = (int)data[i + 1];
        if (b1 >= 65 && b1 <= 90) { b1 = b1 + 32; }
        if (b1 == (byte)115 && data[i + 2] == (byte)118 && data[i + 3] == (byte)103) {  // "svg"
            return true;
        }
        if (b1 == (byte)63 && data[i + 1] == (byte)63) {  // "<?"
            return true;
        }
        // "<!DOCTYPE svg"
        if (data[i + 1] == (byte)33 && data[i + 2] == (byte)68
            && data[i + 3] == (byte)79 && data[i + 4] == (byte)67) {
            return true;
        }
        return false;
    }
}
