// RFC 037 · production-surface §4 · ScrollView 竖滚动条 + 滚轮 Offset（横条 UI 另门禁）。
//
// ScrollView 是滚动视图容器，当内容超出可见区域时显示滚动条。
//
// WPF 同构层级对照：
//   WPF: FrameworkElement → Control → ScrollViewer（WPF 中派生自 ContentControl）
//   Arc:  FrameworkElement → Panel → ScrollView（Arc 简化：归到 Panel 层）
//
// **冲突处理（RFC 037 D1 WPF 同构）**：
//   - Background 已由 Panel 声明——ScrollView 不重复声明，使用继承版本
//   - ScrollView 保留特有 DP：Content/HorizontalScrollBarVisibility/
//     VerticalScrollBarVisibility/HorizontalOffset/VerticalOffset
//
// RFC 037 D1 WPF 同构编程模型：
//   每个公共属性仅由两件套驱动：
//     1. 静态 DependencyProperty<T> 元数据（由 RegisterProperty<T> 工厂创建）
//     2. 属性 wrapper 调用 Element.GetValue<T>/SetValue<T>
//   Signal<T> 后端由 Element 基类内部维护，用户不感知。

namespace Arc.UI.Components.Layout;

using Arc;
using Arc.UI;
using Arc.UI.Layout;

/// <summary>滚动视图容器，当内容超出可见区域时显示滚动条。Background 由 Panel 继承。</summary>
public class ScrollView : Panel {
    /// <summary>构造元素并绑定运行时类型身份（供动态依赖属性解析）。</summary>
    public ScrollView() {
        this.Type = typeof(ScrollView);
    }

    // ===== 静态依赖属性元数据（RFC 037 D1 WPF 同构）=====

    /// <summary>Content 属性元数据——滚动内容（唯一子元素），默认 null。</summary>
    public static DependencyProperty<Element> ContentProperty =
        RegisterProperty<Element>(nameof(Content), typeof(ScrollView), null);

    /// <summary>HorizontalScrollBarVisibility 属性元数据——水平滚动条可见性，默认 Auto。</summary>
    public static DependencyProperty<ScrollBarVisibility> HorizontalScrollBarVisibilityProperty =
        RegisterProperty<ScrollBarVisibility>(nameof(HorizontalScrollBarVisibility), typeof(ScrollView), ScrollBarVisibility.Auto);

    /// <summary>VerticalScrollBarVisibility 属性元数据——垂直滚动条可见性，默认 Auto。</summary>
    public static DependencyProperty<ScrollBarVisibility> VerticalScrollBarVisibilityProperty =
        RegisterProperty<ScrollBarVisibility>(nameof(VerticalScrollBarVisibility), typeof(ScrollView), ScrollBarVisibility.Auto);

    /// <summary>HorizontalOffset 属性元数据——当前水平滚动偏移，默认 0.0。</summary>
    public static DependencyProperty<double> HorizontalOffsetProperty =
        RegisterProperty<double>(nameof(HorizontalOffset), typeof(ScrollView), 0.0);

    /// <summary>VerticalOffset 属性元数据——当前垂直滚动偏移，默认 0.0。</summary>
    public static DependencyProperty<double> VerticalOffsetProperty =
        RegisterProperty<double>(nameof(VerticalOffset), typeof(ScrollView), 0.0);

    // ===== 公共属性 wrapper：委托 Element.GetValue<T>/SetValue<T> =====

    /// <summary>滚动内容（唯一子元素）。</summary>
    public Element Content {
        get { return this.GetValue<Element>(ContentProperty); }
        set { this.SetValue<Element>(ContentProperty, value); }
    }

    /// <summary>水平滚动条可见性。</summary>
    public ScrollBarVisibility HorizontalScrollBarVisibility {
        get { return this.GetValue<ScrollBarVisibility>(HorizontalScrollBarVisibilityProperty); }
        set { this.SetValue<ScrollBarVisibility>(HorizontalScrollBarVisibilityProperty, value); }
    }

    /// <summary>垂直滚动条可见性。</summary>
    public ScrollBarVisibility VerticalScrollBarVisibility {
        get { return this.GetValue<ScrollBarVisibility>(VerticalScrollBarVisibilityProperty); }
        set { this.SetValue<ScrollBarVisibility>(VerticalScrollBarVisibilityProperty, value); }
    }

    /// <summary>当前水平滚动偏移（像素）。</summary>
    public double HorizontalOffset {
        get { return this.GetValue<double>(HorizontalOffsetProperty); }
        set { this.SetValue<double>(HorizontalOffsetProperty, value); }
    }

    /// <summary>当前垂直滚动偏移（像素）。</summary>
    public double VerticalOffset {
        get { return this.GetValue<double>(VerticalOffsetProperty); }
        set { this.SetValue<double>(VerticalOffsetProperty, value); }
    }

    /// <summary>内容总宽度（Measure 后更新）。Draft：无滚动条 UI。</summary>
    public double ExtentWidth {
        get { return _extentWidth; }
    }

    /// <summary>内容总高度（Measure 后更新）。Draft：无滚动条 UI。</summary>
    public double ExtentHeight {
        get { return _extentHeight; }
    }

    /// <summary>可见区域宽度（Arrange 后 RenderSize.Width）。</summary>
    public double ViewportWidth {
        get { return this.RenderWidth; }
    }

    /// <summary>可见区域高度（Arrange 后 RenderSize.Height）。</summary>
    public double ViewportHeight {
        get { return this.RenderHeight; }
    }

    /// <summary>可水平滚动最大偏移（像素）。</summary>
    public double ScrollableWidth {
        get {
            double v = this.ViewportWidth;
            double e = _extentWidth;
            double d = e - v;
            if (d < 0.0) {
                return 0.0;
            }
            return d;
        }
    }

    /// <summary>可垂直滚动最大偏移（像素）。</summary>
    public double ScrollableHeight {
        get {
            double v = this.ViewportHeight;
            double e = _extentHeight;
            double d = e - v;
            if (d < 0.0) {
                return 0.0;
            }
            return d;
        }
    }

    double _extentWidth = 0.0;
    double _extentHeight = 0.0;

    /// <summary>滚轮增量（像素）更新 Offset 并 clamp 到可滚动范围。Disabled 忽略。</summary>
    public void ApplyWheelDelta(double deltaX, double deltaY) {
        if (this.VerticalScrollBarVisibility != ScrollBarVisibility.Disabled) {
            this.SetVerticalOffsetClamped(this.VerticalOffset + deltaY);
        }
        if (this.HorizontalScrollBarVisibility != ScrollBarVisibility.Disabled) {
            this.SetHorizontalOffsetClamped(this.HorizontalOffset + deltaX);
        }
    }

    /// <summary>设置 VerticalOffset 并 clamp 到 [0, ScrollableHeight]。Disabled 不改偏移。</summary>
    public void SetVerticalOffsetClamped(double value) {
        if (this.VerticalScrollBarVisibility == ScrollBarVisibility.Disabled) {
            return;
        }
        if (value < 0.0) {
            value = 0.0;
        }
        double maxV = this.ScrollableHeight;
        if (value > maxV) {
            value = maxV;
        }
        this.VerticalOffset = value;
    }

    /// <summary>设置 HorizontalOffset 并 clamp 到 [0, ScrollableWidth]（无横向条 UI）。Disabled 不改偏移。</summary>
    public void SetHorizontalOffsetClamped(double value) {
        if (this.HorizontalScrollBarVisibility == ScrollBarVisibility.Disabled) {
            return;
        }
        if (value < 0.0) {
            value = 0.0;
        }
        double maxH = this.ScrollableWidth;
        if (value > maxH) {
            value = maxH;
        }
        this.HorizontalOffset = value;
    }

    protected override LayoutSize MeasureOverride(LayoutSize availableSize) {
        FrameworkElement content = this.resolveContent();
        if (content == null) {
            return new LayoutSize(0.0, 0.0);
        }
        double availW = availableSize.Width;
        double availH = availableSize.Height;
        
        // 传递无限约束给内容（允许内容按自身大小测量）
        LayoutSize unbounded = new LayoutSize(LayoutHelper.Unbounded, LayoutHelper.Unbounded);
        LayoutHelper.MeasureChild(content, unbounded);
        LayoutSize d = content.DesiredSize;
        _extentWidth = d.Width;
        _extentHeight = d.Height;
        double w = d.Width;
        double h = d.Height;
        
        // 如果外层有有界约束，则裁剪内容大小
        bool wBounded = availW > 0.0 && availW < 1000000000.0;
        bool hBounded = availH > 0.0 && availH < 1000000000.0;
        if (wBounded && w > availW) {
            w = availW;
        }
        if (hBounded && h > availH) {
            h = availH;
        }
        return new LayoutSize(w, h);
    }

    protected override void ArrangeOverride(LayoutSize finalSize) {
        FrameworkElement content = this.resolveContent();
        if (content == null) {
            return;
        }
        double x = 0.0 - this.HorizontalOffset;
        double y = 0.0 - this.VerticalOffset;

        // 使用内容的期望尺寸，如果为 0 则回退到 Extent 尺寸
        LayoutSize d = content.DesiredSize;
        double contentW = d.Width;
        double contentH = d.Height;
        if (contentW <= 0.0) {
            contentW = _extentWidth;
        }
        if (contentH <= 0.0) {
            contentH = _extentHeight;
        }
        LayoutHelper.ArrangeChild(this, content, x, y, contentW, contentH);
    }

    private FrameworkElement resolveContent() {
        Element c = this.Content;
        if (c != null) {
            return (FrameworkElement)c;
        }
        if (this.Children != null && this.Children.Count > 0) {
            return (FrameworkElement)this.Children[0];
        }
        return null;
    }
}
