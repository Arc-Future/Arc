// RFC 037 D2.1 / D5.1 / RFC 037 D1: Arc.UI.Components.Layout — StackPanel 布局容器。
//
// StackPanel 是栈式布局容器，按水平或垂直方向顺序排列子元素。
//
// WPF 同构层级对照：
//   WPF: FrameworkElement → Panel → StackPanel
//   Arc:  FrameworkElement → Panel → StackPanel
//
// **冲突处理（RFC 037 D1 WPF 同构）**：
//   - Background 已由 Panel 声明——StackPanel 不重复声明，使用继承版本
//   - HorizontalAlignment/VerticalAlignment 已由 FrameworkElement 声明——继承版本
//   - StackPanel 保留特有 DP：Orientation/Spacing
//
// RFC 037 D1 WPF 同构编程模型：
//   每个公共属性仅由两件套驱动：
//     1. 静态 DependencyProperty<T> 元数据（由 RegisterProperty<T> 工厂创建）
//     2. 属性 wrapper 调用 Element.GetValue<T>/SetValue<T>
//   Signal<T> 后端由 Element 基类内部维护，用户不感知。

namespace Arc.UI.Components.Layout;

using Arc.UI.Layout;

/// <summary>栈式布局容器，按水平或垂直方向顺序排列子元素。Background 由 Panel 继承。</summary>
public class StackPanel : Panel {
    /// <summary>构造元素并绑定运行时类型身份（供动态依赖属性解析）。</summary>
    public StackPanel() {
        this.Type = typeof(StackPanel);
    }

    // ===== 静态依赖属性元数据（RFC 037 D1 WPF 同构）=====

    /// <summary>Orientation 属性元数据——排列方向，默认 Vertical。</summary>
    public static DependencyProperty<Orientation> OrientationProperty =
        RegisterProperty<Orientation>(nameof(Orientation), typeof(StackPanel), Orientation.Vertical);

    /// <summary>Spacing 属性元数据——子元素间距，默认 0.0。</summary>
    public static DependencyProperty<double> SpacingProperty =
        RegisterProperty<double>(nameof(Spacing), typeof(StackPanel), 0.0);

    // ===== 公共属性 wrapper：委托 Element.GetValue<T>/SetValue<T> =====

    /// <summary>排列方向：Orientation.Horizontal 或 Orientation.Vertical（默认）。</summary>
    public Orientation Orientation {
        get { return this.GetValue<Orientation>(OrientationProperty); }
        set { this.SetValue<Orientation>(OrientationProperty, value); }
    }

    /// <summary>子元素间距（像素）。</summary>
    public double Spacing {
        get { return this.GetValue<double>(SpacingProperty); }
        set { this.SetValue<double>(SpacingProperty, value); }
    }

    protected override LayoutSize MeasureOverride(LayoutSize availableSize) {
        return Flexbox.MeasureStack(this, this.Orientation, this.Spacing, availableSize);
    }

    protected override void ArrangeOverride(LayoutSize finalSize) {
        Flexbox.ArrangeStack(this, this.Orientation, this.Spacing, finalSize);
    }
}
