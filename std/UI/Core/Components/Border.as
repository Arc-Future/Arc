// RFC 037 · Border 布局装饰（对标 WPF Decorator/Border 最小面）。
//
// Background / BorderBrush / BorderThickness / CornerRadius / Padding / Child。
// 单子：Child DP 或 Children[0]（ARML AddChild 同 ScrollView 先例）。
// 禁 Class/SizeMode/Variant；隐式 Style → Color.Surface/Border + Size.Border.Thickness/Padding + Radius.Control。

namespace Arc.UI.Components;

using Arc.UI;
using Arc.UI.Layout;
using Arc.UI.Media;

/// <summary>边框装饰容器——背景/描边/圆角/内边距包裹单一 Child。</summary>
public class Border : Control {
    /// <summary>BorderBrush 属性元数据——描边画刷。</summary>
    public static DependencyProperty<Brush> BorderBrushProperty =
        RegisterProperty<Brush>(nameof(BorderBrush), typeof(Border),
            new SolidColorBrush(Color.Transparent()));

    /// <summary>BorderThickness 属性元数据——四边描边宽（逗号分隔字符串）。</summary>
    public static DependencyProperty<string> BorderThicknessProperty =
        RegisterProperty<string>(nameof(BorderThickness), typeof(Border), "1");

    /// <summary>CornerRadius 属性元数据——统一圆角（px）。</summary>
    public static DependencyProperty<double> CornerRadiusProperty =
        RegisterProperty<double>(nameof(CornerRadius), typeof(Border), 0.0);

    /// <summary>Padding 属性元数据——内边距（逗号分隔字符串）。</summary>
    public static DependencyProperty<string> PaddingProperty =
        RegisterProperty<string>(nameof(Padding), typeof(Border), "0,0,0,0");

    /// <summary>Child 属性元数据——单一子元素（可选；亦经 Children）。</summary>
    public static DependencyProperty<FrameworkElement> ChildProperty =
        RegisterProperty<FrameworkElement>(nameof(Child), typeof(Border), null);

    public Border() {
        this.Type = typeof(Border);
        this.Focusable = false;
        this.IsTabStop = false;
    }

    /// <summary>描边画刷（string 面兼容 hex）。</summary>
    public string BorderBrush {
        get { return this.GetValue<Brush>(BorderBrushProperty).ToHex(); }
        set { this.SetValue<Brush>(BorderBrushProperty, Brush.FromString(value)); }
    }

    /// <summary>描边厚度（"1" 或 "l,t,r,b"）。</summary>
    public string BorderThickness {
        get { return this.GetValue<string>(BorderThicknessProperty); }
        set { this.SetValue<string>(BorderThicknessProperty, value); }
    }

    /// <summary>统一圆角半径（像素）。</summary>
    public double CornerRadius {
        get { return this.GetValue<double>(CornerRadiusProperty); }
        set { this.SetValue<double>(CornerRadiusProperty, value); }
    }

    /// <summary>内边距（"0" 或 "l,t,r,b"）。</summary>
    public string Padding {
        get { return this.GetValue<string>(PaddingProperty); }
        set { this.SetValue<string>(PaddingProperty, value); }
    }

    /// <summary>单一子元素（与 Children[0] 二选一入口）。</summary>
    public FrameworkElement Child {
        get { return this.GetValue<FrameworkElement>(ChildProperty); }
        set {
            FrameworkElement previous = this.Child;
            if (previous != null && this.Children != null) {
                this.Children.Remove(previous);
            }
            this.SetValue<FrameworkElement>(ChildProperty, value);
            if (value != null) {
                this.AddChild(value);
            }
        }
    }

    Thickness ResolveBorderThickness() {
        return Thickness.Parse(this.BorderThickness).Sanitized();
    }

    Thickness ResolvePadding() {
        return Thickness.Parse(this.Padding).Sanitized();
    }

    FrameworkElement ResolveChild() {
        FrameworkElement c = this.Child;
        if (c != null) {
            return c;
        }
        if (this.Children != null && this.Children.Count > 0) {
            return (FrameworkElement)this.Children[0];
        }
        return null;
    }

    protected override LayoutSize MeasureOverride(LayoutSize availableSize) {
        Thickness bt = this.ResolveBorderThickness();
        Thickness pad = this.ResolvePadding();
        double insetW = bt.Left + bt.Right + pad.Left + pad.Right;
        double insetH = bt.Top + bt.Bottom + pad.Top + pad.Bottom;
        FrameworkElement child = this.ResolveChild();
        double childW = 0.0;
        double childH = 0.0;
        if (child != null) {
            double availW = availableSize.Width;
            double availH = availableSize.Height;
            if (availW > 0.0 && availW < LayoutHelper.Unbounded) {
                availW = availW - insetW;
                if (availW < 0.0) {
                    availW = 0.0;
                }
            }
            if (availH > 0.0 && availH < LayoutHelper.Unbounded) {
                availH = availH - insetH;
                if (availH < 0.0) {
                    availH = 0.0;
                }
            }
            LayoutHelper.MeasureChild(child, new LayoutSize(availW, availH));
            LayoutSize d = child.DesiredSize;
            childW = d.Width;
            childH = d.Height;
        }
        double w = childW + insetW;
        double h = childH + insetH;
        if (this.Width > 0.0) {
            w = this.Width;
        }
        if (this.Height > 0.0) {
            h = this.Height;
        }
        double boundW = availableSize.Width;
        if (boundW > 0.0 && boundW < LayoutHelper.Unbounded && w > boundW) {
            w = boundW;
        }
        double boundH = availableSize.Height;
        if (boundH > 0.0 && boundH < LayoutHelper.Unbounded && h > boundH) {
            h = boundH;
        }
        return new LayoutSize(w, h);
    }

    protected override void ArrangeOverride(LayoutSize finalSize) {
        Thickness bt = this.ResolveBorderThickness();
        Thickness pad = this.ResolvePadding();
        FrameworkElement child = this.ResolveChild();
        if (child == null) {
            return;
        }
        double x = bt.Left + pad.Left;
        double y = bt.Top + pad.Top;
        double slotW = finalSize.Width - bt.Left - bt.Right - pad.Left - pad.Right;
        double slotH = finalSize.Height - bt.Top - bt.Bottom - pad.Top - pad.Bottom;
        if (slotW < 0.0) {
            slotW = 0.0;
        }
        if (slotH < 0.0) {
            slotH = 0.0;
        }
        LayoutHelper.ArrangeChild(this, child, x, y, slotW, slotH);
    }
}
