// Arc.UI — Stretch 缩放模式强类型枚举（对标 WPF Stretch，用于 Image.Stretch）。

namespace Arc.UI;

/// <summary>内容缩放模式（对标 WPF `Stretch`）。成员顺序对齐 WPF。</summary>
public enum Stretch {
    /// <summary>不缩放，保留原始尺寸。</summary>
    None,
    /// <summary>等比缩放填满（可能变形）。</summary>
    Fill,
    /// <summary>等比缩放，完整显示且不变形。</summary>
    Uniform,
    /// <summary>等比缩放填满并裁剪溢出。</summary>
    UniformToFill,
}
