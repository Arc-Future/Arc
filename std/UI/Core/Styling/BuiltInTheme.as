// RFC 037 §3 + RFC 037 D3.10 — Arc.UI.Styling 内置主题资源（WPF ResourceDictionary）。
//
// 配合 [RFC 037 §4](../../../docs/rfc/037-ui.md) 立宪：
// 内置控件默认 Theme 的资源字典须以 key→ResourceValue 声明，禁止硬编码终态。
//
// 设计（对标 WPF ResourceDictionary + 键覆盖定制 + IBrush 系列体系）：
//   - 色值唯一权威源：`std/UI/Themes/{Light,Dark}.arml`（UI-P2）；经 arc-ui 生成
//     `BuiltInTheme.Colors.g.as` → `BuiltInThemeColors.Fill*Colors`，本类仅保留键名
//     常量 + 几何/motion + 薄工厂（CreateLight/CreateDark）。
//   - 颜色以**类型化 Color/SolidColorBrush** 注册（加载期一次解析），替代 hex 字符串
//     逐帧 DecodeHexColor 的运行时开销；键以 `const string` 承载防拼写错。
//   - 几何/深度（CornerRadius / Thickness / Elevation）为**编译期结构化常量**，
//     不落入资源字典（主题无关），供 VSM/ControlVisual 与渲染器直接取值。
//   - 用户定制主题只需以同键覆盖（本地条目经 MergedDictionaries 优先于活动主题）；
//     arml `{StaticResource key}` 引用即编译期/加载期常量，无歧义。
//
// canonical 值见 crates/runtime-ui/platform/common/rt_ui_design_tokens.h
// （Light 默认值保持对齐；Dark 为 Arc 侧资源，头文件不镜像——wgpu 唯一后端）。

namespace Arc.UI.Styling;

using Arc.Collections;
using Arc.UI.Layout;
using Arc.UI.Media;

/// <summary>内置 Light/Dark 主题资源键 + 默认资源字典 + 结构化几何/深度常量。</summary>
internal class BuiltInTheme {
    // =====================================================================
    // §0 结构化几何/深度常量（编译期固化，主题无关；VSM/渲染器直接取值）
    // =====================================================================

    /// <summary>控件圆角（Button/TextBox/Slider 轨道等）。</summary>
    public static CornerRadius ControlRadius() {
        return new CornerRadius(6.0);
    }

    /// <summary>卡片/面板圆角。</summary>
    public static CornerRadius SurfaceRadius() {
        return new CornerRadius(8.0);
    }

    /// <summary>胶囊圆角（标签/Chip）。</summary>
    public static CornerRadius PillRadius() {
        return new CornerRadius(999.0);
    }

    /// <summary>控件发丝边框（1px）。</summary>
    public static Thickness ControlBorderWidth() {
        return new Thickness(1.0);
    }

    /// <summary>焦点辉光宽度（px）。</summary>
    public const double FocusRingWidth = 2.0;

    /// <summary>hover 状态过渡时长（ms，快而跟手）。</summary>
    public const double MotionHoverMs = 120.0;

    /// <summary>pressed 状态过渡时长（ms，最跟手、反应最快）。</summary>
    public const double MotionPressMs = 90.0;

    /// <summary>focus 状态过渡时长（ms，更从容显从容感）。</summary>
    public const double MotionFocusMs = 160.0;

    /// <summary>hover 抬升深度（软阴影）。</summary>
    public static Elevation HoverLift() {
        return new Elevation(6.0, 10.0, 2.0, 0.18);
    }

    /// <summary>pressed 内陷深度（阴影收拢，模拟下沉）。</summary>
    public static Elevation PressedLift() {
        return new Elevation(6.0, 4.0, 0.0, 0.10);
    }

    /// <summary>focus 焦点辉光深度。</summary>
    public static Elevation FocusGlow() {
        return new Elevation(6.0, 8.0, 0.0, 0.35);
    }

    // =====================================================================
    // §1 Color —— 字符串键（消费方经 Application.Current.ResolveColor/ResolveBrush 解析）
    // =====================================================================

    public const string Background = "Color.Background";
    public const string Surface = "Color.Surface";
    public const string Border = "Color.Border";
    public const string TextPrimary = "Color.Text.Primary";
    public const string TextSecondary = "Color.Text.Secondary";
    public const string Primary = "Color.Primary";
    public const string PrimaryHover = "Color.Primary.Hover";
    public const string PrimaryPressed = "Color.Primary.Pressed";
    public const string FocusRing = "Color.Focus.Ring";
    public const string DisabledFill = "Color.Disabled.Fill";
    public const string DisabledText = "Color.Disabled.Text";
    public const string TextOnAccent = "Color.Text.OnAccent";
    public const string Transparent = "Color.Transparent";
    public const string SurfaceHover = "Color.Surface.Hover";
    public const string SurfaceStripe = "Color.Surface.Stripe";
    public const string SliderTrack = "Color.Slider.Track";
    public const string ScrollTrack = "Color.Scroll.Track";
    public const string ScrollThumb = "Color.Scroll.Thumb";
    public const string ScrollThumbHover = "Color.Scroll.Thumb.Hover";
    public const string ScrollThumbActive = "Color.Scroll.Thumb.Active";
    public const string Placeholder = "Color.Placeholder";
    public const string Overlay = "Color.Overlay";
    public const string Negative = "Color.Negative";
    public const string AccentGradientA = "Color.Accent.Gradient.A";
    public const string AccentGradientB = "Color.Accent.Gradient.B";

    // §2 Radius
    public const string RadiusControl = "Radius.Control";
    public const string RadiusSurface = "Radius.Surface";
    public const string RadiusPill = "Radius.Pill";

    // §3 Spacing (8-grid)
    public const string SpacingXS = "Spacing.XS";
    public const string SpacingSM = "Spacing.SM";
    public const string SpacingMD = "Spacing.MD";
    public const string SpacingLG = "Spacing.LG";
    public const string SpacingXL = "Spacing.XL";

    // §4 Typography
    public const string FontBodySize = "Font.Body.Size";
    public const string FontBodyFamily = "Font.Body.Family";
    public const string FontCaptionSize = "Font.Caption.Size";
    public const string FontHeadingSize = "Font.Heading.Size";

    // §5 Border / Motion
    public const string MotionDurationFast = "Motion.Duration.Fast";
    public const string MotionDurationNormal = "Motion.Duration.Normal";

    /// <summary>非色 token（两主题共享）。</summary>
    private static void FillNonColor(ResourceDictionary d) {
        d.Add(BuiltInTheme.RadiusControl, 6.0);
        d.Add(BuiltInTheme.RadiusSurface, 8.0);
        d.Add(BuiltInTheme.RadiusPill, 999.0);
        d.Add(BuiltInTheme.SpacingXS, 4.0);
        d.Add(BuiltInTheme.SpacingSM, 8.0);
        d.Add(BuiltInTheme.SpacingMD, 12.0);
        d.Add(BuiltInTheme.SpacingLG, 16.0);
        d.Add(BuiltInTheme.SpacingXL, 24.0);
        d.Add(BuiltInTheme.FontBodySize, 14.0);
        d.Add(BuiltInTheme.FontBodyFamily, "Segoe UI");
        d.Add(BuiltInTheme.FontCaptionSize, 12.0);
        d.Add(BuiltInTheme.FontHeadingSize, 16.0);
        d.Add(BuiltInTheme.MotionDurationFast, 120.0);
        d.Add(BuiltInTheme.MotionDurationNormal, 160.0);
    }

    /// <summary>Light 默认主题资源字典（RFC 037 §3 Light；色值来自 Themes/Light.arml）。</summary>
    public static ResourceDictionary CreateLight() {
        ResourceDictionary d = new ResourceDictionary();
        BuiltInThemeColors.FillLightColors(d);
        BuiltInTheme.FillNonColor(d);
        return d;
    }

    /// <summary>Dark 默认主题资源字典（同 key 集；色值来自 Themes/Dark.arml）。</summary>
    public static ResourceDictionary CreateDark() {
        ResourceDictionary d = new ResourceDictionary();
        BuiltInThemeColors.FillDarkColors(d);
        BuiltInTheme.FillNonColor(d);
        return d;
    }
}
