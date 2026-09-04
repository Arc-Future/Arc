namespace Arc.Drawing;

/// <summary>
/// 32 位 ARGB 颜色。纯 Arc 实现（byte 通道字段），无需 Builtin。
/// M1 像素缓冲为原生 RGBA8 句柄，打包/解包统一走 long 无符号算术
/// （int 打包 0x80 及以上通道值会溢出为负数，经 i64 ABI 传参需符号扩展，
/// 故 <see cref="ToArgb"/> 返回 long——如实反映 ABI 语义，与 C# int 返回不同）。
/// </summary>
public struct RgbColor {

    public byte A { get; }
    public byte R { get; }
    public byte G { get; }
    public byte B { get; }

    public RgbColor(byte a, byte r, byte g, byte b) {
        this.A = a;
        this.R = r;
        this.G = g;
        this.B = b;
    }

    /// <summary>不透明 ARGB（alpha = 255）。</summary>
    public static RgbColor FromArgb(byte r, byte g, byte b) {
        return new RgbColor((byte)255, r, g, b);
    }

    /// <summary>显式 alpha 的 ARGB。</summary>
    public static RgbColor FromArgb(byte a, byte r, byte g, byte b) {
        return new RgbColor(a, r, g, b);
    }

    /// <summary>
    /// 打包为 long 形态 ARGB-32（0x00AARRGGBB）。返回 long 而非 int：
    /// 通道值 ≥ 0x80 时 int 打包会溢出为负数，long 形态与 rt_image_* ABI 一致。
    /// </summary>
    public long ToArgb() {
        return (long)this.A * (long)16777216 + (long)this.R * (long)65536
            + (long)this.G * (long)256 + (long)this.B;
    }
}
