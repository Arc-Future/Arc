// RFC 037 D5.3: Arc.UI.Layout — CanvasLayout 绝对定位布局算法（layout-v2）。

namespace Arc.UI.Layout;

using Arc.UI;
using Arc.UI.Components.Layout;

/// <summary>Canvas 绝对定位布局。</summary>
internal class CanvasLayout {
    public static LayoutSize Measure(Canvas panel, LayoutSize available) {
        if (panel == null || panel.Children == null) {
            return new LayoutSize(0.0, 0.0);
        }
        double availW = available.Width;
        double availH = available.Height;
        double maxRight = 0.0;
        double maxBottom = 0.0;
        int count = panel.Children.Count;
        for (int i = 0; i < count; i++) {
            Element raw = panel.Children[i];
            FrameworkElement child = (FrameworkElement)raw;
            LayoutHelper.MeasureChild(child, new LayoutSize(availW, availH));
            double left = LayoutHelper.GetAttachedNumber(raw, Canvas.LeftProperty, 0.0);
            double top = LayoutHelper.GetAttachedNumber(raw, Canvas.TopProperty, 0.0);
            double right = maxRight;
            double bottom = maxBottom;
            if (left + child.DesiredSize.Width > right) {
                right = left + child.DesiredSize.Width;
            }
            if (top + child.DesiredSize.Height > bottom) {
                bottom = top + child.DesiredSize.Height;
            }
            maxRight = right;
            maxBottom = bottom;
        }
        if (panel.Width > 0.0) {
            maxRight = panel.Width;
        }
        if (panel.Height > 0.0) {
            maxBottom = panel.Height;
        }
        return new LayoutSize(maxRight, maxBottom);
    }

    public static void Arrange(Canvas panel, LayoutSize finalSize) {
        if (panel == null || panel.Children == null) {
            return;
        }
        int count = panel.Children.Count;
        for (int i = 0; i < count; i++) {
            Element raw = panel.Children[i];
            FrameworkElement child = (FrameworkElement)raw;
            double left = LayoutHelper.GetAttachedNumber(raw, Canvas.LeftProperty, 0.0);
            double top = LayoutHelper.GetAttachedNumber(raw, Canvas.TopProperty, 0.0);
            double w = child.DesiredSize.Width;
            double h = child.DesiredSize.Height;
            if (child.Width > 0.0) {
                w = child.Width;
            }
            if (child.Height > 0.0) {
                h = child.Height;
            }
            LayoutHelper.ArrangeChild(panel, child, left, top, w, h);
        }
    }
}
