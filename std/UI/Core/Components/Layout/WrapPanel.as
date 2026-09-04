// RFC 037 D2.1 / RFC 037 D1: Arc.UI.Components.Layout — WrapPanel 自动换行布局。
//
// WrapPanel 是自动换行布局容器，子元素按方向排列，超出宽度时换行。
//
// WPF 同构层级对照：
//   WPF: FrameworkElement → Panel → WrapPanel
//   Arc:  FrameworkElement → Panel → WrapPanel
//
// **冲突处理（RFC 037 D1 WPF 同构）**：
//   - Background 已由 Panel 声明——WrapPanel 不重复声明，使用继承版本
//   - WrapPanel 保留特有 DP：Orientation/ItemWidth/ItemHeight
//
// RFC 037 D1 WPF 同构编程模型：
//   每个公共属性仅由两件套驱动：
//     1. 静态 DependencyProperty<T> 元数据（由 RegisterProperty<T> 工厂创建）
//     2. 属性 wrapper 调用 Element.GetValue<T>/SetValue<T>
//   Signal<T> 后端由 Element 基类内部维护，用户不感知。

namespace Arc.UI.Components.Layout;

using Arc.UI.Layout;

/// <summary>自动换行布局容器，子元素按方向排列，超出宽度时换行。Background 由 Panel 继承。</summary>
public class WrapPanel : Panel {
    /// <summary>构造元素并绑定运行时类型身份（供动态依赖属性解析）。</summary>
    public WrapPanel() {
        this.Type = typeof(WrapPanel);
    }

    // ===== 静态依赖属性元数据（RFC 037 D1 WPF 同构）=====

    /// <summary>Orientation 属性元数据——排列方向，默认 Horizontal。</summary>
    public static DependencyProperty<Orientation> OrientationProperty =
        RegisterProperty<Orientation>(nameof(Orientation), typeof(WrapPanel), Orientation.Horizontal);

    /// <summary>ItemWidth 属性元数据——子元素统一宽度，默认 0.0（自动）。</summary>
    public static DependencyProperty<double> ItemWidthProperty =
        RegisterProperty<double>(nameof(ItemWidth), typeof(WrapPanel), 0.0);

    /// <summary>ItemHeight 属性元数据——子元素统一高度，默认 0.0（自动）。</summary>
    public static DependencyProperty<double> ItemHeightProperty =
        RegisterProperty<double>(nameof(ItemHeight), typeof(WrapPanel), 0.0);

    // ===== 公共属性 wrapper：委托 Element.GetValue<T>/SetValue<T> =====

    /// <summary>排列方向：Orientation.Horizontal（默认）或 Orientation.Vertical。</summary>
    public Orientation Orientation {
        get { return this.GetValue<Orientation>(OrientationProperty); }
        set { this.SetValue<Orientation>(OrientationProperty, value); }
    }

    /// <summary>子元素统一宽度（0 表示自动）。</summary>
    public double ItemWidth {
        get { return this.GetValue<double>(ItemWidthProperty); }
        set { this.SetValue<double>(ItemWidthProperty, value); }
    }

    /// <summary>子元素统一高度（0 表示自动）。</summary>
    public double ItemHeight {
        get { return this.GetValue<double>(ItemHeightProperty); }
        set { this.SetValue<double>(ItemHeightProperty, value); }
    }

    protected override LayoutSize MeasureOverride(LayoutSize availableSize) {
        return Flexbox.MeasureWrap(this, this.Orientation, this.ItemWidth, this.ItemHeight,
            0.0, availableSize);
    }

    protected override void ArrangeOverride(LayoutSize finalSize) {
        Flexbox.ArrangeWrap(this, this.Orientation, this.ItemWidth, this.ItemHeight,
            0.0, finalSize);
    }
}
