// Arc.UI — ScrollBarVisibility 滚动条可见性强类型枚举（对标 WPF ScrollBarVisibility）。

namespace Arc.UI;

/// <summary>滚动条可见性（对标 WPF `ScrollBarVisibility`）。成员顺序对齐 WPF。</summary>
public enum ScrollBarVisibility {
    /// <summary>即使内容溢出也禁用滚动条。</summary>
    Disabled,
    /// <summary>内容溢出时自动显示滚动条。</summary>
    Auto,
    /// <summary>始终隐藏滚动条，但仍可滚动。</summary>
    Hidden,
    /// <summary>始终显示滚动条。</summary>
    Visible,
}
