// RFC 037 D2.1 / RFC 037 D6: Arc.UI — Shape 图形基类。
//
// Shape 是 WPF 同构层级中的「图形元素」——所有几何图形（Rectangle/Ellipse/
// Line/Path/Polyline/Polygon）的父类。在 FrameworkElement 之上扩展 Fill
// （填充画刷）、Stroke（描边画刷）、StrokeThickness（描边粗细）等
// 「图形级」DP。
//
// WPF 同构层级对照：
//   WPF: FrameworkElement → Shape → Rectangle/Ellipse/Line/Path/...
//   Arc:  FrameworkElement → Shape → Rectangle（M3+ 扩展 Ellipse/Line/Path）
//
// Shape 与 Control 的区别：
//   - Control 用于交互控件（按钮/输入框/滑块等），有 Template/IsEnabled 概念
//   - Shape 用于装饰图形（矩形/圆形/线条等），仅用于绘制，无交互语义
//   - Shape 不派生自 Control——WPF 中 Shape 派生自 FrameworkElement 而非 Control
//
// **命名空间归属**：本文件位于 std/UI/Markup/ 子目录，但归属到 `Arc.UI`
// 根命名空间（基类放根命名空间原则）。Shape 是所有图形元素的父类，
// 必须在 `Arc.UI` 根命名空间，使派生类（Arc.UI.Components.Rectangle 等）
// 只需 `using Arc.UI;` 即可访问。

namespace Arc.UI;

using Arc.UI.Media;

/// <summary>
/// 图形基类——扩展 FrameworkElement 添加 Fill/Stroke/StrokeThickness 等
/// 图形级 DP。
/// </summary>
public class Shape : FrameworkElement {
    // ===== 静态依赖属性元数据（RFC 037 D1 WPF 同构）=====

    /// <summary>Fill 属性元数据——填充画刷（类型化 Brush；默认白）。</summary>
    public static DependencyProperty<Brush> FillProperty =
        RegisterProperty<Brush>(nameof(Fill), typeof(Shape), new SolidColorBrush(Color.White()));

    /// <summary>Stroke 属性元数据——描边画刷（类型化 Brush；默认黑）。</summary>
    public static DependencyProperty<Brush> StrokeProperty =
        RegisterProperty<Brush>(nameof(Stroke), typeof(Shape), new SolidColorBrush(Color.Parse("#FF000000")));

    /// <summary>StrokeThickness 属性元数据——描边粗细（px）。</summary>
    public static DependencyProperty<double> StrokeThicknessProperty =
        RegisterProperty<double>(nameof(StrokeThickness), typeof(Shape), 1.0);

    // ===== 公共属性 wrapper：委托 Element.GetValue<T>/SetValue<T> =====

    /// <summary>填充画刷（string 面兼容：hex/命名色；内部 DP 存类型化 Brush）。</summary>
    public string Fill {
        get { return this.GetValue<Brush>(FillProperty).ToHex(); }
        set { this.SetValue<Brush>(FillProperty, Brush.FromString(value)); }
    }

    /// <summary>描边画刷（string 面兼容：hex/命名色；内部 DP 存类型化 Brush）。</summary>
    public string Stroke {
        get { return this.GetValue<Brush>(StrokeProperty).ToHex(); }
        set { this.SetValue<Brush>(StrokeProperty, Brush.FromString(value)); }
    }

    /// <summary>描边粗细（px）。</summary>
    public double StrokeThickness {
        get { return this.GetValue<double>(StrokeThicknessProperty); }
        set { this.SetValue<double>(StrokeThicknessProperty, value); }
    }
}
