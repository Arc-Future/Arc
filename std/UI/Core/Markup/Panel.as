// RFC 037 D2.1 / D5.1 / RFC 037 D6: Arc.UI — Panel 布局面板基类。
//
// Panel 是 WPF 同构层级中的「面板层」——所有布局容器（StackPanel/Canvas/
// Grid/DockPanel/WrapPanel/ScrollView）的父类。在 FrameworkElement 之上
// 扩展 Background（背景画刷）与 Children 集合（复用 Element.Children）。
//
// WPF 同构层级对照：
//   WPF: FrameworkElement → Panel → StackPanel/Canvas/Grid/DockPanel/...
//   Arc:  FrameworkElement → Panel → StackPanel/Canvas/Grid/DockPanel/...
//
// Panel 不引入「Children DP」——WPF 中 Panel.Children 是
/// UIElementCollection（带内部版本号的派生集合），Arc 复用 Element.Children
// （public List<Element>）即可。子元素由 codegen 通过 AddChild 添加。
//
// 布局系统两阶段（Measure/Arrange）当前未实现——Arc 不支持元组返回类型
// `(T1, T2)`，待 Size 结构体就位后引入正式布局 API（M3+ 独立 RFC）。
//
// **命名空间归属**：本文件位于 std/UI/Markup/ 子目录，但归属到 `Arc.UI`
// 根命名空间（基类放根命名空间原则）。Panel 是所有 Layout 容器的父类，
// 必须在 `Arc.UI` 根命名空间，使派生类（Arc.UI.Components.Layout.*）
// 只需 `using Arc.UI;` 即可访问。

namespace Arc.UI;

using Arc.UI.Media;

/// <summary>
/// 布局面板基类——所有布局容器的父类。扩展 FrameworkElement 添加 Background
/// DP。布局两阶段钩子（Measure/Arrange）待 Size 结构体就位后引入。
/// </summary>
public class Panel : FrameworkElement {
    /// <summary>构造元素并绑定运行时类型身份（供动态依赖属性解析）。</summary>
    public Panel() {
        this.Type = typeof(Panel);
    }

    // ===== 静态依赖属性元数据（RFC 037 D1 WPF 同构）=====

    /// <summary>Background 属性元数据——面板背景画刷（类型化 Brush；默认透明）。</summary>
    public static DependencyProperty<Brush> BackgroundProperty =
        RegisterProperty<Brush>(nameof(Background), typeof(Panel), new SolidColorBrush(Color.Transparent()));

    // ===== 公共属性 wrapper：委托 Element.GetValue<T>/SetValue<T> =====

    /// <summary>面板背景画刷（string 面兼容：hex/命名色；内部 DP 存类型化 Brush）。</summary>
    public string Background {
        get { return this.GetValue<Brush>(BackgroundProperty).ToHex(); }
        set { this.SetValue<Brush>(BackgroundProperty, Brush.FromString(value)); }
    }
}
