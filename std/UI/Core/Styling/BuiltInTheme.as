// RFC 037 §3 + RFC 037 D3.10 — Arc.UI.Styling 内置主题资源（WPF ResourceDictionary）。
//
// 配合 [RFC 037 §4](../../../docs/rfc/037-ui.md) 立宪：
// 内置控件默认 Theme 的资源字典须以 key→ResourceValue 声明，禁止硬编码终态。
//
// **双对齐口径（诚实 · 唯一）**：
//   - 交互语义 / API / 布局：对标 WPF（单一惯用法）
//   - 默认视觉皮肤：对标 Ant Design **6.x** 公开 Seed/Map Token（colorPrimary=#1677ff 等；
//     权威 https://ant.design/docs/react/customize-theme）；**不**宣称像素级 DOM/CSS 复刻
//   - 禁止 Fluent/Material 混搭默认色；禁止「antd 像素克隆」第二口径
//
// 设计：
//   - 色值唯一权威源：`std/UI/Core/Themes/{Light,Dark}.arml`（UI-P2）；经 arc-ui 生成
//     `BuiltInTheme.Colors.g.as` → `BuiltInThemeColors.Fill*Colors`，本类仅保留键名
//     常量 + 几何/motion + 薄工厂（CreateLight/CreateDark）。
//   - 隐式 Style 权威源：`Themes/Controls.arml` + `Themes/Controls/*.arml` →
//     `BuiltInTheme.Styles.g.as` → `CreateControls()`；CreateLight/Dark 经
//     `MergedDictionaries.Add` 并入（与 WPF 主题合并同构；切主题 O(1) 换整份 RD）。
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

// ControlMetrics：尺寸/圆角/字号数值权威（本文件仅键名 + 工厂）。

/// <summary>内置 Light/Dark 主题资源键 + 默认资源字典 + 结构化几何/深度常量。</summary>
internal class BuiltInTheme {
    // =====================================================================
    // §0 结构化几何/深度常量（编译期固化，主题无关；VSM/渲染器直接取值）
    // =====================================================================

    /// <summary>控件圆角（Button/TextBox/Slider 轨道等；= ControlMetrics.ControlRadius）。</summary>
    public static CornerRadius ControlRadius() {
        return new CornerRadius(ControlMetrics.ControlRadius);
    }

    /// <summary>卡片/面板圆角（= ControlMetrics.SurfaceRadius）。</summary>
    public static CornerRadius SurfaceRadius() {
        return new CornerRadius(ControlMetrics.SurfaceRadius);
    }

    /// <summary>胶囊圆角（标签/Chip）。</summary>
    public static CornerRadius PillRadius() {
        return new CornerRadius(ControlMetrics.PillRadius);
    }

    /// <summary>控件发丝边框（1px）。</summary>
    public static Thickness ControlBorderWidth() {
        return new Thickness(ControlMetrics.BorderWidth);
    }

    /// <summary>焦点辉光宽度（px；= ControlMetrics.FocusRingWidth）。</summary>
    public const double FocusRingWidth = 2.0;

    /// <summary>hover 状态过渡时长（ms，快而跟手）。</summary>
    public const double MotionHoverMs = 120.0;

    /// <summary>pressed 状态过渡时长（ms，最跟手、反应最快）。</summary>
    public const double MotionPressMs = 90.0;

    /// <summary>focus 状态过渡时长（ms，更从容显从容感）。</summary>
    public const double MotionFocusMs = 160.0;

    /// <summary>hover 抬升深度（Ant 偏平；轻阴影）。</summary>
    public static Elevation HoverLift() {
        return new Elevation(4.0, 6.0, 1.0, 0.12);
    }

    /// <summary>pressed 内陷深度（阴影收拢）。</summary>
    public static Elevation PressedLift() {
        return new Elevation(2.0, 2.0, 0.0, 0.08);
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
    /// <summary>Ant colorBorderDisabled（6.x Map Token；禁用描边）。</summary>
    public const string BorderDisabled = "Color.Border.Disabled";
    public const string TextPrimary = "Color.Text.Primary";
    public const string TextSecondary = "Color.Text.Secondary";
    public const string Primary = "Color.Primary";
    public const string PrimaryHover = "Color.Primary.Hover";
    public const string PrimaryPressed = "Color.Primary.Pressed";
    public const string FocusRing = "Color.Focus.Ring";
    public const string DisabledFill = "Color.Disabled.Fill";
    public const string DisabledText = "Color.Disabled.Text";
    public const string TextOnAccent = "Color.Text.OnAccent";
    /// <summary>输入占位文字色（Ant colorTextPlaceholder）。</summary>
    public const string TextPlaceholder = "Color.Text.Placeholder";
    public const string Transparent = "Color.Transparent";
    public const string SurfaceHover = "Color.Surface.Hover";
    public const string SurfaceStripe = "Color.Surface.Stripe";
    public const string SliderTrack = "Color.Slider.Track";
    public const string ScrollTrack = "Color.Scroll.Track";
    public const string ScrollThumb = "Color.Scroll.Thumb";
    public const string ScrollThumbHover = "Color.Scroll.Thumb.Hover";
    public const string ScrollThumbPressed = "Color.Scroll.Thumb.Pressed";
    public const string Overlay = "Color.Overlay";
    /// <summary>Ant colorError；与 Style 短键 Danger 成套。</summary>
    public const string Danger = "Color.Danger";
    /// <summary>Ant colorErrorHover。</summary>
    public const string DangerHover = "Color.Danger.Hover";
    /// <summary>Ant colorErrorActive。</summary>
    public const string DangerPressed = "Color.Danger.Pressed";
    public const string Success = "Color.Success";
    public const string Warning = "Color.Warning";
    /// <summary>文本选区填充（Ant colorPrimary @ 25%）。</summary>
    public const string TextSelection = "Color.Text.Selection";
    /// <summary>Image 无纹理占位底。</summary>
    public const string ImageFill = "Color.Image.Fill";
    /// <summary>Image 无纹理占位描边。</summary>
    public const string ImageBorder = "Color.Image.Border";
    /// <summary>DrawText 高亮底（编辑器/选区高亮）。</summary>
    public const string TextHighlight = "Color.Text.Highlight";

    // §2 Radius / Size（数值权威 = ControlMetrics；字典供 ResolveNumber）
    public const string RadiusControl = "Radius.Control";
    public const string RadiusSurface = "Radius.Surface";
    public const string RadiusPill = "Radius.Pill";
    public const string ControlHeightSM = "Size.Control.Height.SM";
    public const string ControlHeight = "Size.Control.Height";
    public const string ControlHeightLG = "Size.Control.Height.LG";
    /// <summary>Button/Toggle 中档 Thickness 字符串（L,T,R,B；溯源 ControlMetrics）。</summary>
    public const string ControlPadding = "Size.Control.Padding";
    /// <summary>小档 Thickness（Ant size=small · paddingInlineSM）。</summary>
    public const string ControlPaddingSM = "Size.Control.Padding.SM";
    /// <summary>大档 Thickness（Ant size=large · paddingInlineLG）。</summary>
    public const string ControlPaddingLG = "Size.Control.Padding.LG";
    /// <summary>Border 发丝描边 Thickness（L,T,R,B；= ControlMetrics.BorderWidth）。</summary>
    public const string BorderThickness = "Size.Border.Thickness";
    /// <summary>Border 默认内边距 Thickness（= Spacing.MD 四边）。</summary>
    public const string BorderPadding = "Size.Border.Padding";
    /// <summary>ProgressBar 轨高（Ant line；= ControlMetrics.ProgressBarThickness）。</summary>
    public const string ProgressThickness = "Size.Progress.Thickness";
    /// <summary>Slider 默认测高（= ControlMetrics.SliderDefaultHeight）。</summary>
    public const string SliderHeight = "Size.Slider.Height";

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
    /// <summary>标准缓动（Ant motionEaseOut 近似；MotionEngine 默认消费）。</summary>
    public const string MotionEasingStandard = "Motion.Easing.Standard";
    /// <summary>线性缓动 token 键。</summary>
    public const string MotionEasingLinear = "Motion.Easing.Linear";
    /// <summary>ease-in cubic token 键。</summary>
    public const string MotionEasingIn = "Motion.Easing.In";
    /// <summary>ease-in-out cubic token 键。</summary>
    public const string MotionEasingInOut = "Motion.Easing.InOut";

    /// <summary>缓动曲线字面量（资源值；与 MotionEngine.Ease* 族对齐）。</summary>
    public const string MotionCurveEaseOut = "ease-out";
    public const string MotionCurveLinear = "linear";
    public const string MotionCurveEaseIn = "ease-in";
    public const string MotionCurveEaseInOut = "ease-in-out";

    /// <summary>非色 token（两主题共享；尺寸/圆角/字号溯源 ControlMetrics）。</summary>
    private static void FillNonColor(ResourceDictionary d) {
        d.Add(BuiltInTheme.RadiusControl, ControlMetrics.ControlRadius);
        d.Add(BuiltInTheme.RadiusSurface, ControlMetrics.SurfaceRadius);
        d.Add(BuiltInTheme.RadiusPill, ControlMetrics.PillRadius);
        d.Add(BuiltInTheme.ControlHeightSM, ControlMetrics.ControlHeightSM);
        d.Add(BuiltInTheme.ControlHeight, ControlMetrics.ControlHeight);
        d.Add(BuiltInTheme.ControlHeightLG, ControlMetrics.ControlHeightLG);
        d.Add(BuiltInTheme.ControlPaddingSM, BuiltInTheme.FormatPaddingThickness(
            ControlMetrics.ButtonPaddingXSM, ControlMetrics.ButtonPaddingYSM));
        d.Add(BuiltInTheme.ControlPadding, BuiltInTheme.FormatPaddingThickness(
            ControlMetrics.ButtonPaddingX, ControlMetrics.ButtonPaddingY));
        d.Add(BuiltInTheme.ControlPaddingLG, BuiltInTheme.FormatPaddingThickness(
            ControlMetrics.ButtonPaddingXLG, ControlMetrics.ButtonPaddingYLG));
        d.Add(BuiltInTheme.BorderThickness, BuiltInTheme.FormatPaddingThickness(
            ControlMetrics.BorderWidth, ControlMetrics.BorderWidth));
        d.Add(BuiltInTheme.BorderPadding, BuiltInTheme.FormatPaddingThickness(
            ControlMetrics.SpacingMD, ControlMetrics.SpacingMD));
        d.Add(BuiltInTheme.ProgressThickness, ControlMetrics.ProgressBarThickness);
        d.Add(BuiltInTheme.SliderHeight, ControlMetrics.SliderDefaultHeight);
        d.Add(BuiltInTheme.SpacingXS, ControlMetrics.SpacingXS);
        d.Add(BuiltInTheme.SpacingSM, ControlMetrics.SpacingSM);
        d.Add(BuiltInTheme.SpacingMD, ControlMetrics.SpacingMD);
        d.Add(BuiltInTheme.SpacingLG, ControlMetrics.SpacingLG);
        d.Add(BuiltInTheme.SpacingXL, ControlMetrics.SpacingXL);
        d.Add(BuiltInTheme.FontBodySize, ControlMetrics.FontBodySize);
        d.Add(BuiltInTheme.FontBodyFamily, "Segoe UI");
        d.Add(BuiltInTheme.FontCaptionSize, ControlMetrics.FontCaptionSize);
        d.Add(BuiltInTheme.FontHeadingSize, ControlMetrics.FontHeadingSize);
        d.Add(BuiltInTheme.MotionDurationFast, BuiltInTheme.MotionHoverMs);
        d.Add(BuiltInTheme.MotionDurationNormal, BuiltInTheme.MotionFocusMs);
        d.Add(BuiltInTheme.MotionEasingStandard, BuiltInTheme.MotionCurveEaseOut);
        d.Add(BuiltInTheme.MotionEasingLinear, BuiltInTheme.MotionCurveLinear);
        d.Add(BuiltInTheme.MotionEasingIn, BuiltInTheme.MotionCurveEaseIn);
        d.Add(BuiltInTheme.MotionEasingInOut, BuiltInTheme.MotionCurveEaseInOut);
    }

    /// <summary>将水平/垂直内边距合成 Thickness 字符串（L,T,R,B）。</summary>
    private static string FormatPaddingThickness(double padX, double padY) {
        return padX.ToString() + "," + padY.ToString() + "," + padX.ToString() + "," + padY.ToString();
    }

    /// <summary>
    /// 控件隐式 Style 字典（来自 Themes/Controls.arml 生成物；chrome Setter，字体禁入）。
    /// </summary>
    public static ResourceDictionary CreateControls() {
        return BuiltInThemeStyles.Create();
    }

    /// <summary>
    /// 将内置隐式 Style 并入主题字典（真实 MergedDictionaries；非空壳索引）。
    /// CreateLight/CreateDark 与自定义主题基底共用。
    /// </summary>
    public static void AddImplicitStyles(ResourceDictionary d) {
        if (d == null) {
            return;
        }
        ResourceDictionary controls = BuiltInTheme.CreateControls();
        DefaultControlTemplates.AttachToStyles(controls);
        d.MergedDictionaries.Add(controls);
    }

    /// <summary>Light 默认主题资源字典（RFC 037 §3 Light；色值来自 Themes/Light.arml）。</summary>
    public static ResourceDictionary CreateLight() {
        ResourceDictionary d = new ResourceDictionary();
        BuiltInThemeColors.FillLightColors(d);
        BuiltInTheme.FillNonColor(d);
        BuiltInTheme.AddImplicitStyles(d);
        return d;
    }

    /// <summary>Dark 默认主题资源字典（同 key 集；色值来自 Themes/Dark.arml）。</summary>
    public static ResourceDictionary CreateDark() {
        ResourceDictionary d = new ResourceDictionary();
        BuiltInThemeColors.FillDarkColors(d);
        BuiltInTheme.FillNonColor(d);
        BuiltInTheme.AddImplicitStyles(d);
        return d;
    }
}
