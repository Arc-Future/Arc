// RFC 037 D2.1 / RFC 037 D6: Arc.UI — Control 控件基类。
//
// Control 是 WPF 同构层级中的「控件层」——在 FrameworkElement 之上扩展
// 外观（Background/Foreground/字体属性）、交互（IsEnabled）、模板（Template）
// 等「控件级」语义。所有用户可交互的元素均派生自 Control 或其子类
// （InputElement/ContentControl/Button/CheckBox/Slider/TextBox 等）。
//
// WPF 同构层级对照：
//   WPF: FrameworkElement → Control → ContentControl → Button/CheckBox
//                                       ContentControl → Window/UserControl/Page
//                          → Control → Slider/TextBox (无 Content 概念)
//   Arc:  FrameworkElement → Control → InputElement → ContentControl → Button/CheckBox/Window/...
//                          → Control → InputElement → Slider/TextBox/TextBlock (无 Content 概念)
//
// Control 承载的 WPF 同构 DP：
//   - Background：背景画刷（颜色字符串或 Brush 对象）
//   - Foreground：前景画刷（颜色字符串或 Brush 对象）
//   - FontFamily：字体族（"Segoe UI"/"Microsoft YaHei" 等）
//   - FontSize：字体大小
//   - FontWeight：字体粗细（"Normal"/"Bold" 等）
//   - IsEnabled：是否启用（false 时灰显且不响应输入）
//   - Template：ControlTemplate（自定义控件外观）
//   - Focusable：是否可获取焦点
//   - IsTabStop：是否参与 Tab 导航
//
// **命名空间归属**：本文件位于 std/UI/Markup/ 子目录，但归属到 `Arc.UI`
// 根命名空间（基类放根命名空间原则）。Control 是 InputElement（进而
// ContentControl 与所有交互控件 Slider/TextBox 等）与 TextBlock 的父类，
// 必须在 `Arc.UI` 根命名空间。

namespace Arc.UI;

using Arc.UI.Media;

/// <summary>
/// 控件基类——扩展 FrameworkElement 添加外观、交互、模板等控件级 DP。
/// </summary>
public class Control : FrameworkElement {
    /// <summary>构造元素并绑定运行时类型身份（供动态依赖属性解析）。</summary>
    public Control() {
        this.Type = typeof(Control);
    }

    // ===== 静态依赖属性元数据（RFC 037 D1 WPF 同构）=====

    /// <summary>Background 属性元数据——背景画刷（类型化 Brush；默认透明）。</summary>
    public static DependencyProperty<Brush> BackgroundProperty =
        RegisterProperty<Brush>(nameof(Background), typeof(Control), new SolidColorBrush(Color.Transparent()));

    /// <summary>Foreground 属性元数据——前景画刷（类型化 Brush；默认白）。
    /// 环境属性（RFC 037 §4 元数据声明）：沿元素树继承。</summary>
    public static DependencyProperty<Brush> ForegroundProperty =
        RegisterInheritedProperty<Brush>(nameof(Foreground), typeof(Control), new SolidColorBrush(Color.White()));

    /// <summary>FontFamily 属性元数据——字体族。环境属性（元数据声明）：沿
    /// 元素树继承，Window/根容器设置一次全树生效（全局字体默认单一源即本 DP 默认值）。</summary>
    public static DependencyProperty<string> FontFamilyProperty =
        RegisterInheritedProperty<string>(nameof(FontFamily), typeof(Control), "Segoe UI");

    /// <summary>FontSize 属性元数据——字体大小（px）。环境属性（元数据声明）：沿元素树继承。</summary>
    public static DependencyProperty<double> FontSizeProperty =
        RegisterInheritedProperty<double>(nameof(FontSize), typeof(Control), 14.0);

    /// <summary>FontWeight 属性元数据——字体粗细。环境属性（元数据声明）：沿元素树继承。</summary>
    public static DependencyProperty<string> FontWeightProperty =
        RegisterInheritedProperty<string>(nameof(FontWeight), typeof(Control), "Normal");

    /// <summary>IsEnabled 属性元数据——是否启用。</summary>
    public static DependencyProperty<bool> IsEnabledProperty =
        RegisterProperty<bool>(nameof(IsEnabled), typeof(Control), true);

    /// <summary>Template 属性元数据——ControlTemplate 自定义外观。</summary>
    public static DependencyProperty<object> TemplateProperty =
        RegisterProperty<object>(nameof(Template), typeof(Control), null);

    /// <summary>Focusable 属性元数据——是否可获取焦点。</summary>
    public static DependencyProperty<bool> FocusableProperty =
        RegisterProperty<bool>(nameof(Focusable), typeof(Control), false);

    /// <summary>IsTabStop 属性元数据——是否参与 Tab 导航。</summary>
    public static DependencyProperty<bool> IsTabStopProperty =
        RegisterProperty<bool>(nameof(IsTabStop), typeof(Control), false);

    // ===== 公共属性 wrapper：委托 Element.GetValue<T>/SetValue<T> =====

    /// <summary>背景画刷（string 面兼容：hex/命名色；内部 DP 存类型化 Brush）。</summary>
    public string Background {
        get { return this.GetValue<Brush>(BackgroundProperty).ToHex(); }
        set { this.SetValue<Brush>(BackgroundProperty, Brush.FromString(value)); }
    }

    /// <summary>前景画刷（string 面兼容：hex/命名色；内部 DP 存类型化 Brush）。</summary>
    public string Foreground {
        get { return this.GetValue<Brush>(ForegroundProperty).ToHex(); }
        set { this.SetValue<Brush>(ForegroundProperty, Brush.FromString(value)); }
    }

    /// <summary>字体族（如 "Segoe UI"/"Microsoft YaHei"）。</summary>
    public string FontFamily {
        get { return this.GetValue<string>(FontFamilyProperty); }
        set { this.SetValue<string>(FontFamilyProperty, value); }
    }

    /// <summary>字体大小（px）。</summary>
    public double FontSize {
        get { return this.GetValue<double>(FontSizeProperty); }
        set { this.SetValue<double>(FontSizeProperty, value); }
    }

    /// <summary>字体粗细："Normal"/"Bold"/"Light" 等。</summary>
    public string FontWeight {
        get { return this.GetValue<string>(FontWeightProperty); }
        set { this.SetValue<string>(FontWeightProperty, value); }
    }

    /// <summary>是否启用（false 时灰显且不响应输入）。</summary>
    public bool IsEnabled {
        get { return this.GetValue<bool>(IsEnabledProperty); }
        set { this.SetValue<bool>(IsEnabledProperty, value); }
    }

    /// <summary>ControlTemplate 自定义外观。</summary>
    public object Template {
        get { return this.GetValue<object>(TemplateProperty); }
        set { this.SetValue<object>(TemplateProperty, value); }
    }

    /// <summary>是否可获取焦点。</summary>
    public bool Focusable {
        get { return this.GetValue<bool>(FocusableProperty); }
        set { this.SetValue<bool>(FocusableProperty, value); }
    }

    /// <summary>是否参与 Tab 导航。</summary>
    public bool IsTabStop {
        get { return this.GetValue<bool>(IsTabStopProperty); }
        set { this.SetValue<bool>(IsTabStopProperty, value); }
    }
}
