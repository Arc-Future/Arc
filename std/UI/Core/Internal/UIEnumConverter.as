// Arc.UI.Internal — UIEnumConverter 枚举 ↔ 字符串转换（内部辅助）。
//
// 服务两处内部边界：
//   - PlatformTreeSync：将 Arc DP 的强类型枚举序列化为平台镜像字符串（ElementSetString）
//   - StyleEvaluator：将 markup 样式 Setter 的字符串解析为强类型枚举 DP 值
//
// **访问权限**：`internal`——纯内部实现，绝不对开发者暴露。强类型枚举本身是公共
// API（对标 WPF），但「枚举 ↔ 显示文本」的往返转换细节应封装在内部，不进入
// 开发者编码面（RFC 036 宣称纪律 · 访问权限控制）。

namespace Arc.UI.Internal;

using Arc.UI;

/// <summary>UI 枚举 ↔ 字符串 转换。内部使用，不公开。</summary>
internal static class UIEnumConverter {
    // ===== Orientation =====

    /// <summary>序列化为平台镜像文本。</summary>
    public static string OrientationText(Orientation o) {
        if (o == Orientation.Vertical) {
            return "Vertical";
        }
        return "Horizontal";
    }

    /// <summary>从 markup 字符串解析；未知值回退 Horizontal。</summary>
    public static Orientation ParseOrientation(string s) {
        if (s == "Vertical") {
            return Orientation.Vertical;
        }
        return Orientation.Horizontal;
    }

    // ===== HorizontalAlignment / VerticalAlignment =====

    public static string HorizontalAlignmentText(HorizontalAlignment a) {
        if (a == HorizontalAlignment.Left) { return "Left"; }
        if (a == HorizontalAlignment.Center) { return "Center"; }
        if (a == HorizontalAlignment.Right) { return "Right"; }
        return "Stretch";
    }

    public static HorizontalAlignment ParseHorizontalAlignment(string s) {
        if (s == "Left") { return HorizontalAlignment.Left; }
        if (s == "Center") { return HorizontalAlignment.Center; }
        if (s == "Right") { return HorizontalAlignment.Right; }
        return HorizontalAlignment.Stretch;
    }

    public static string VerticalAlignmentText(VerticalAlignment a) {
        if (a == VerticalAlignment.Top) { return "Top"; }
        if (a == VerticalAlignment.Center) { return "Center"; }
        if (a == VerticalAlignment.Bottom) { return "Bottom"; }
        return "Stretch";
    }

    public static VerticalAlignment ParseVerticalAlignment(string s) {
        if (s == "Top") { return VerticalAlignment.Top; }
        if (s == "Center") { return VerticalAlignment.Center; }
        if (s == "Bottom") { return VerticalAlignment.Bottom; }
        return VerticalAlignment.Stretch;
    }

    // ===== Stretch =====

    public static string StretchText(Stretch s) {
        if (s == Stretch.None) { return "None"; }
        if (s == Stretch.Fill) { return "Fill"; }
        if (s == Stretch.Uniform) { return "Uniform"; }
        return "UniformToFill";
    }

    public static Stretch ParseStretch(string s) {
        if (s == "None") { return Stretch.None; }
        if (s == "Fill") { return Stretch.Fill; }
        if (s == "Uniform") { return Stretch.Uniform; }
        return Stretch.UniformToFill;
    }

    // ===== ScrollBarVisibility =====

    public static string ScrollBarVisibilityText(ScrollBarVisibility v) {
        if (v == ScrollBarVisibility.Disabled) { return "Disabled"; }
        if (v == ScrollBarVisibility.Auto) { return "Auto"; }
        if (v == ScrollBarVisibility.Hidden) { return "Hidden"; }
        return "Visible";
    }

    public static ScrollBarVisibility ParseScrollBarVisibility(string s) {
        if (s == "Disabled") { return ScrollBarVisibility.Disabled; }
        if (s == "Auto") { return ScrollBarVisibility.Auto; }
        if (s == "Hidden") { return ScrollBarVisibility.Hidden; }
        return ScrollBarVisibility.Visible;
    }
}
