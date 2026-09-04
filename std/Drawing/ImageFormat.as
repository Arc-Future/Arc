namespace Arc.Drawing;

/// <summary>图像编码格式。M1 已接线 PNG / JPEG；Bmp / Tga 仅占位（stb 已支持，
/// 但 M1 未暴露 ABI，Save 遇之抛 NotSupportedException）。</summary>
public enum ImageFormat {
    Png,
    Jpeg,
    Bmp,
    Tga,
}
