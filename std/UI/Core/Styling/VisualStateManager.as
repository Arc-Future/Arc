// RFC 037 §1/§6 · RFC 037 D3.10: VisualStateManager —— 状态→视觉配方唯一化（生产级）。
//
// 所有控件的交互态视觉（hover / pressed / focus / disabled / checked / selected）
// 由本表统一解析为**视觉配方 ControlVisual**，禁止在渲染器/控件内硬编码终态。
//
// 生产级设计要点：
//   - **强类型状态**：以 <see cref="ControlState"/> 承载各交互轴，杜绝裸 int 传参的
//     顺序错位（剔除「半成品」最大的隐患）。状态按 WPF VisualStateGroup 语义归为
//     独立轴（CommonStates / FocusStates / CheckStates / SelectionStates），方法内组合。
//   - **三层解析契约**（对标 WPF DynamicResource + 编译期固化）：
//       颜色角色 → 资源键（string），渲染器经 Application.Current ResolveColor/ResolveBrush
//                 动态解析；切主题即全链生效，用户 arml 覆盖经 MergedDictionaries 本地优先。
//       几何角色 → CornerRadius / Thickness，编译期结构化常量（BuiltInTheme），无运行时查找。
//       深度角色 → Elevation（hover 抬升 / pressed 内陷 / focus 辉光），DrawSurfaceShadow 落地。
//   - **现代视觉配方**：渐变色对（GradientStart/GradientEnd → DrawLinearGradient+radius，同一 SDF）
//     与每状态过渡时长（MotionDuration，MotionEngine.ResolveColorDur 消费），达成
//     「现代感 / 未来感 / 体验感舒适」的动效层次。
//
// 主题定制（顺序即优先级，对标 WPF ResourceDictionary）：
//   1. **键覆盖（局部定制）**：应用 `Resources` 本地条目优先于活动主题——用户仅需以
//      同键写入新值（纯色 SolidColorBrush 或渐变 LinearGradientBrush）即可定制任意控件。
//   2. **主题覆盖（整份主题）**：
//      `Application.Current.ThemeDictionaries.RegisterTheme(name, dict)` 注册，再
//      `Application.Current.SwitchTheme(name)` 生效；RegisterTheme 仅纯存储（编译期已聚合平坦字典）。
//   3. **内置明暗**：`Application.Current.SwitchTheme("Light"/"Dark")` 即全链切换。
//
// canonical 见 crates/runtime-ui/platform/common/rt_ui_design_tokens.h。

namespace Arc.UI.Styling;

using Arc.UI.Layout;
using Arc.UI.Media;

/// <summary>控件交互状态（强类型，各轴独立；对标 WPF VisualStateGroup 组合）。</summary>
internal struct ControlState {
    /// <summary>是否启用（CommonStates.Disabled）。</summary>
    public int Enabled;

    /// <summary>鼠标悬停（CommonStates.Hover）。</summary>
    public int Hover;

    /// <summary>按下（CommonStates.Pressed）。</summary>
    public int Pressed;

    /// <summary>焦点态（FocusStates.Focused）。</summary>
    public int Focused;

    /// <summary>勾选/选中切换态（CheckStates.Checked）。</summary>
    public int Checked;

    /// <summary>列表选中态（SelectionStates.Selected）。</summary>
    public int Selected;

    /// <summary>按轴构造状态（缺省轴为 0）。</summary>
    public static ControlState Of(int enabled, int hover, int pressed, int focused,
                                  int checked_, int selected) {
        ControlState s = new ControlState();
        s.Enabled = enabled;
        s.Hover = hover;
        s.Pressed = pressed;
        s.Focused = focused;
        s.Checked = checked_;
        s.Selected = selected;
        return s;
    }
}

/// <summary>
/// 控件状态解析出的视觉配方（颜色角色=资源键，几何/深度=结构化常量；
/// 渲染器解析 + 插值后上屏）。
/// </summary>
internal struct ControlVisual {
    /// <summary>主填充（按钮底 / 输入底 / 勾选底）。</summary>
    public string Background;

    /// <summary>前景文字。</summary>
    public string Foreground;

    /// <summary>边框。</summary>
    public string Border;

    /// <summary>焦点外晕。</summary>
    public string FocusRing;

    /// <summary>强调色（Slider 填充 / 勾选标记 / 滚动条 thumb / Tab 指示条）。</summary>
    public string Accent;

    /// <summary>强调面上的文字（Primary 按钮 / 选中勾选内文字）。</summary>
    public string AccentText;

    /// <summary>轨道色（Slider 轨迹 / Progress 底托 / ComboBox 下拉底）。</summary>
    public string Track;

    /// <summary>thumb 色（滚动条 thumb / 离散滑块）。</summary>
    public string Thumb;

    /// <summary>占位文字色（TextBox placeholder）。</summary>
    public string Placeholder;

    /// <summary>浮层蒙层（Tooltip / 弹层 scrim）。</summary>
    public string Overlay;

    /// <summary>现代渐变强调起点（渲染器经 DrawLinearGradient 消费）。</summary>
    public string GradientStart;

    /// <summary>现代渐变强调终点。</summary>
    public string GradientEnd;

    /// <summary>几何：圆角（四角结构）。</summary>
    public CornerRadius Radius;

    /// <summary>几何：边框宽（四边结构）。</summary>
    public Thickness BorderWidth;

    /// <summary>深度：当前抬升/内陷（hover/pressed 反馈）。</summary>
    public Elevation Lift;

    /// <summary>深度：焦点辉光。</summary>
    public Elevation FocusGlow;

    /// <summary>几何：焦点环宽度（px）。</summary>
    public double FocusRingWidth;

    /// <summary>本状态的过渡时长（ms；0=瞬达，负值回退角色默认）；供 MotionEngine 消费。</summary>
    public double MotionDuration;

    /// <summary>构造默认配方（几何/深度取内置常量，颜色角色待状态填充）。</summary>
    public static ControlVisual Base() {
        ControlVisual v = new ControlVisual();
        v.Radius = BuiltInTheme.ControlRadius();
        v.BorderWidth = BuiltInTheme.ControlBorderWidth();
        v.Lift = Elevation.None();
        v.FocusGlow = BuiltInTheme.FocusGlow();
        v.FocusRingWidth = BuiltInTheme.FocusRingWidth;
        v.MotionDuration = -1.0;
        return v;
    }
}

/// <summary>状态→视觉配方解析器（RFC 037 §1 态反馈唯一来源；纯映射，不含解析）。</summary>
internal class VisualStateManager {
    private VisualStateManager() {
    }

    /// <summary>禁用态公共降级（各控件共用，避免重复）。</summary>
    private static ControlVisual Disabled(ControlVisual v) {
        v.Background = BuiltInTheme.DisabledFill;
        v.Foreground = BuiltInTheme.DisabledText;
        v.Border = BuiltInTheme.Border;
        v.FocusRing = BuiltInTheme.Transparent;
        v.Accent = BuiltInTheme.DisabledText;
        v.Track = BuiltInTheme.DisabledFill;
        v.Lift = Elevation.None();
        v.MotionDuration = 0.0;
        return v;
    }

    /// <summary>Primary 主按钮：disabled 优先，其次 pressed/hover，默认 Primary；现代渐变强调。</summary>
    public static ControlVisual PrimaryButton(ControlState s) {
        ControlVisual v = ControlVisual.Base();
        if (s.Enabled == 0) {
            return VisualStateManager.Disabled(v);
        }
        if (s.Pressed != 0) {
            v.Background = BuiltInTheme.PrimaryPressed;
            v.Lift = BuiltInTheme.PressedLift();
            v.MotionDuration = BuiltInTheme.MotionPressMs;
        } else if (s.Hover != 0) {
            v.Background = BuiltInTheme.PrimaryHover;
            v.Lift = BuiltInTheme.HoverLift();
            v.MotionDuration = BuiltInTheme.MotionHoverMs;
        } else {
            v.Background = BuiltInTheme.Primary;
            v.MotionDuration = 0.0;
        }
        v.Foreground = BuiltInTheme.TextOnAccent;
        v.AccentText = BuiltInTheme.TextOnAccent;
        v.Border = BuiltInTheme.Transparent;
        v.FocusRing = s.Focused != 0 ? BuiltInTheme.FocusRing : BuiltInTheme.Transparent;
        v.Accent = BuiltInTheme.Primary;
        // 现代渐变强调（渲染器经 DrawLinearGradient 消费；缺渐变键回退 Background 纯色）。
        v.GradientStart = BuiltInTheme.AccentGradientA;
        v.GradientEnd = BuiltInTheme.AccentGradientB;
        return v;
    }

    /// <summary>Ghost 次级按钮：Surface 底 + 边框，hover 提亮边框。</summary>
    public static ControlVisual GhostButton(ControlState s) {
        ControlVisual v = ControlVisual.Base();
        if (s.Enabled == 0) {
            return VisualStateManager.Disabled(v);
        }
        if (s.Pressed != 0) {
            v.Background = BuiltInTheme.SurfaceHover;
            v.Lift = BuiltInTheme.PressedLift();
            v.MotionDuration = BuiltInTheme.MotionPressMs;
        } else if (s.Hover != 0) {
            v.Background = BuiltInTheme.SurfaceHover;
            v.Lift = BuiltInTheme.HoverLift();
            v.MotionDuration = BuiltInTheme.MotionHoverMs;
        } else {
            v.Background = BuiltInTheme.Surface;
            v.MotionDuration = 0.0;
        }
        v.Foreground = BuiltInTheme.TextPrimary;
        v.Border = s.Hover != 0 ? BuiltInTheme.PrimaryHover : BuiltInTheme.Border;
        v.FocusRing = s.Focused != 0 ? BuiltInTheme.FocusRing : BuiltInTheme.Transparent;
        v.Accent = BuiltInTheme.Primary;
        return v;
    }

    /// <summary>Button 聚合入口：按语义选择 Primary 或 Ghost（默认 Primary，RFC 037 §2 主按钮）。</summary>
    public static ControlVisual Button(ControlState s) {
        return VisualStateManager.PrimaryButton(s);
    }

    /// <summary>ToggleButton / CheckBox / Radio：checked 用强调，disabled 降级。</summary>
    public static ControlVisual Toggle(ControlState s) {
        ControlVisual v = ControlVisual.Base();
        if (s.Enabled == 0) {
            return VisualStateManager.Disabled(v);
        }
        if (s.Checked != 0) {
            v.Background = s.Pressed != 0 ? BuiltInTheme.PrimaryPressed
                       : (s.Hover != 0 ? BuiltInTheme.PrimaryHover : BuiltInTheme.Primary);
            v.Border = BuiltInTheme.Transparent;
            v.Accent = BuiltInTheme.TextOnAccent;
            v.AccentText = BuiltInTheme.TextOnAccent;
            v.Lift = s.Pressed != 0 ? BuiltInTheme.PressedLift() : BuiltInTheme.HoverLift();
            v.MotionDuration = s.Pressed != 0 ? BuiltInTheme.MotionPressMs : BuiltInTheme.MotionHoverMs;
            v.GradientStart = BuiltInTheme.AccentGradientA;
            v.GradientEnd = BuiltInTheme.AccentGradientB;
        } else {
            v.Background = s.Pressed != 0 || s.Hover != 0
                ? BuiltInTheme.SurfaceHover
                : BuiltInTheme.Surface;
            v.Border = s.Hover != 0
                ? BuiltInTheme.PrimaryHover
                : BuiltInTheme.Border;
            v.Accent = BuiltInTheme.Primary;
            v.Track = BuiltInTheme.Surface;
            v.MotionDuration = s.Hover != 0 ? BuiltInTheme.MotionHoverMs : 0.0;
        }
        v.Foreground = BuiltInTheme.TextPrimary;
        v.FocusRing = s.Focused != 0 ? BuiltInTheme.FocusRing : BuiltInTheme.Transparent;
        return v;
    }

    /// <summary>TextBox：focus 边框高亮 + 辉光，disabled 降级。</summary>
    public static ControlVisual TextBox(ControlState s) {
        ControlVisual v = ControlVisual.Base();
        if (s.Enabled == 0) {
            return VisualStateManager.Disabled(v);
        }
        v.Background = BuiltInTheme.Surface;
        v.Foreground = BuiltInTheme.TextPrimary;
        v.Border = s.Focused != 0 ? BuiltInTheme.Primary : BuiltInTheme.Border;
        v.FocusRing = s.Focused != 0 ? BuiltInTheme.FocusRing : BuiltInTheme.Transparent;
        v.Accent = BuiltInTheme.Primary;
        v.Placeholder = BuiltInTheme.Placeholder;
        v.MotionDuration = s.Focused != 0 ? BuiltInTheme.MotionFocusMs : 0.0;
        return v;
    }

    /// <summary>Slider：轨道 Track，填充/thumb 用 Accent，disabled 降级。</summary>
    public static ControlVisual Slider(ControlState s) {
        ControlVisual v = ControlVisual.Base();
        if (s.Enabled == 0) {
            return VisualStateManager.Disabled(v);
        }
        v.Background = BuiltInTheme.Surface;
        v.Foreground = BuiltInTheme.TextPrimary;
        v.Border = BuiltInTheme.Border;
        v.Track = BuiltInTheme.SliderTrack;
        v.Accent = s.Pressed != 0 ? BuiltInTheme.PrimaryPressed
                 : (s.Hover != 0 ? BuiltInTheme.PrimaryHover : BuiltInTheme.Primary);
        v.Thumb = BuiltInTheme.Surface;
        v.FocusRing = s.Focused != 0 ? BuiltInTheme.FocusRing : BuiltInTheme.Transparent;
        v.Lift = s.Pressed != 0 ? BuiltInTheme.PressedLift() : BuiltInTheme.HoverLift();
        v.MotionDuration = s.Pressed != 0 ? BuiltInTheme.MotionPressMs
                         : (s.Hover != 0 ? BuiltInTheme.MotionHoverMs : 0.0);
        return v;
    }

    /// <summary>竖滚动条：轨道 subtle 灰底，thumb 可交互加深（hover/pressed 提亮），圆角矩形。</summary>
    public static ControlVisual ScrollBar(ControlState s) {
        ControlVisual v = ControlVisual.Base();
        v.Radius = new CornerRadius(4.0);
        v.Background = BuiltInTheme.ScrollTrack;
        v.Foreground = BuiltInTheme.TextPrimary;
        v.Border = BuiltInTheme.Transparent;
        v.FocusRing = BuiltInTheme.Transparent;
        // Pressed 状态使用更深的颜色（Active），Hover 次之，默认最浅。
        string thumbColor = BuiltInTheme.ScrollThumb;
        if (s.Pressed != 0) {
            thumbColor = BuiltInTheme.ScrollThumbActive;
        } else if (s.Hover != 0) {
            thumbColor = BuiltInTheme.ScrollThumbHover;
        }
        v.Accent = thumbColor;
        v.Thumb = thumbColor;
        v.Track = BuiltInTheme.ScrollTrack;
        v.MotionDuration = (s.Hover != 0 || s.Pressed != 0) ? BuiltInTheme.MotionHoverMs : 0.0;
        return v;
    }

    /// <summary>Progress：轨道 Track，填充 Accent。</summary>
    public static ControlVisual Progress(ControlState s) {
        ControlVisual v = ControlVisual.Base();
        v.Track = BuiltInTheme.SliderTrack;
        v.Accent = s.Enabled == 0 ? BuiltInTheme.DisabledText : BuiltInTheme.Primary;
        v.Border = BuiltInTheme.Transparent;
        v.FocusRing = BuiltInTheme.Transparent;
        return v;
    }

    /// <summary>ComboBox：TextBox 底 + 下拉 caret 强调（hover 提亮边框）。</summary>
    public static ControlVisual ComboBox(ControlState s) {
        ControlVisual v = VisualStateManager.TextBox(s);
        if (s.Enabled != 0 && s.Hover != 0 && s.Focused == 0) {
            v.Border = BuiltInTheme.PrimaryHover;
        }
        return v;
    }

    /// <summary>Tab：选中用强调指示条 + 文字高亮，hover 提亮。</summary>
    public static ControlVisual Tab(ControlState s) {
        ControlVisual v = ControlVisual.Base();
        v.Radius = BuiltInTheme.ControlRadius();
        if (s.Enabled == 0) {
            v.Background = BuiltInTheme.Transparent;
            v.Foreground = BuiltInTheme.DisabledText;
            v.Accent = BuiltInTheme.Transparent;
            return v;
        }
        v.Background = s.Selected != 0 ? BuiltInTheme.Surface : BuiltInTheme.Transparent;
        v.Foreground = s.Selected != 0 ? BuiltInTheme.TextPrimary : BuiltInTheme.TextSecondary;
        v.Accent = s.Selected != 0 ? BuiltInTheme.Primary : BuiltInTheme.Transparent;
        v.Border = BuiltInTheme.Transparent;
        if (s.Hover != 0 && s.Selected == 0) {
            v.Foreground = BuiltInTheme.TextPrimary;
            v.Background = BuiltInTheme.SurfaceHover;
        }
        return v;
    }

    /// <summary>DataGridRow：选中用 Accent 整行填充 + OnAccent 文本（管理后台语义），hover 提亮。</summary>
    public static ControlVisual DataGridRow(ControlState s) {
        ControlVisual v = ControlVisual.Base();
        v.Radius = new CornerRadius(0.0);
        if (s.Enabled == 0) {
            v.Background = BuiltInTheme.Transparent;
            v.Foreground = BuiltInTheme.DisabledText;
            v.Accent = BuiltInTheme.Transparent;
            return v;
        }
        v.Background = s.Selected != 0 ? BuiltInTheme.Primary : BuiltInTheme.Transparent;
        v.Foreground = s.Selected != 0 ? BuiltInTheme.TextOnAccent : BuiltInTheme.TextPrimary;
        v.Accent = s.Selected != 0 ? BuiltInTheme.Primary : BuiltInTheme.Transparent;
        v.Border = BuiltInTheme.Transparent;
        if (s.Hover != 0 && s.Selected == 0) {
            v.Background = BuiltInTheme.SurfaceHover;
        }
        return v;
    }

    /// <summary>ListBoxItem：选中用强调底 + Accent 条，hover 提亮。</summary>
    public static ControlVisual ListBoxItem(ControlState s) {
        ControlVisual v = ControlVisual.Base();
        v.Radius = BuiltInTheme.ControlRadius();
        if (s.Enabled == 0) {
            v.Background = BuiltInTheme.Transparent;
            v.Foreground = BuiltInTheme.DisabledText;
            v.Accent = BuiltInTheme.Transparent;
            return v;
        }
        v.Background = s.Selected != 0 ? BuiltInTheme.SurfaceHover : BuiltInTheme.Transparent;
        v.Foreground = s.Selected != 0 ? BuiltInTheme.TextPrimary : BuiltInTheme.TextSecondary;
        v.Accent = s.Selected != 0 ? BuiltInTheme.Primary : BuiltInTheme.Transparent;
        v.Border = BuiltInTheme.Transparent;
        if (s.Hover != 0 && s.Selected == 0) {
            v.Background = BuiltInTheme.SurfaceHover;
        }
        return v;
    }

    /// <summary>Tooltip：浮层面 + 阴影。</summary>
    public static ControlVisual Tooltip() {
        ControlVisual v = ControlVisual.Base();
        v.Radius = BuiltInTheme.SurfaceRadius();
        v.Background = BuiltInTheme.Surface;
        v.Foreground = BuiltInTheme.TextPrimary;
        v.Border = BuiltInTheme.Border;
        v.FocusRing = BuiltInTheme.Transparent;
        v.Accent = BuiltInTheme.Primary;
        v.Overlay = BuiltInTheme.Overlay;
        v.Lift = new Elevation(8.0, 12.0, 3.0, 0.22);
        return v;
    }
}