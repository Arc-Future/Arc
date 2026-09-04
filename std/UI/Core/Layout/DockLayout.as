// RFC 037 D5: Arc.UI.Layout — DockLayout 停靠布局算法（layout-v2）。

namespace Arc.UI.Layout;

using Arc.UI;
using Arc.UI.Components.Layout;

/// <summary>DockPanel 停靠布局。</summary>
internal class DockLayout {
    public static LayoutSize Measure(DockPanel panel, LayoutSize available) {
        if (panel == null || panel.Children == null) {
            return new LayoutSize(0.0, 0.0);
        }
        double w = 0.0;
        double h = 0.0;
        double remainW = available.Width;
        double remainH = available.Height;
        int count = panel.Children.Count;
        for (int i = 0; i < count; i++) {
            Element raw = panel.Children[i];
            FrameworkElement child = (FrameworkElement)raw;
            string dock = LayoutHelper.GetAttachedString(raw, DockPanel.DockProperty, "Left");
            bool isLast = (i == count - 1) && panel.LastChildFill;
            LayoutSize childAvail = new LayoutSize(remainW, remainH);
            if (dock == "Top" || dock == "Bottom") {
                childAvail.Width = remainW;
                childAvail.Height = LayoutHelper.Unbounded;
            } else if (dock == "Left" || dock == "Right") {
                childAvail.Width = LayoutHelper.Unbounded;
                childAvail.Height = remainH;
            }
            if (isLast) {
                childAvail.Width = remainW;
                childAvail.Height = remainH;
            }
            LayoutHelper.MeasureChild(child, childAvail);
            LayoutSize d = child.DesiredSize;
            if (dock == "Top") {
                remainH -= d.Height;
                if (w < d.Width) { w = d.Width; }
                h += d.Height;
            } else if (dock == "Bottom") {
                remainH -= d.Height;
                if (w < d.Width) { w = d.Width; }
                h += d.Height;
            } else if (dock == "Right") {
                remainW -= d.Width;
                if (h < d.Height) { h = d.Height; }
                w += d.Width;
            } else {
                remainW -= d.Width;
                if (h < d.Height) { h = d.Height; }
                w += d.Width;
            }
        }
        return new LayoutSize(w, h);
    }

    public static void Arrange(DockPanel panel, LayoutSize finalSize) {
        if (panel == null || panel.Children == null) {
            return;
        }
        double x = 0.0;
        double y = 0.0;
        double remainW = finalSize.Width;
        double remainH = finalSize.Height;
        int count = panel.Children.Count;
        for (int i = 0; i < count; i++) {
            Element raw = panel.Children[i];
            FrameworkElement child = (FrameworkElement)raw;
            string dock = LayoutHelper.GetAttachedString(raw, DockPanel.DockProperty, "Left");
            bool isLast = (i == count - 1) && panel.LastChildFill;
            double slotX = 0.0;
            double slotY = 0.0;
            double slotW = 0.0;
            double slotH = 0.0;
            if (isLast) {
                slotX = x;
                slotY = y;
                slotW = remainW;
                slotH = remainH;
            } else if (dock == "Top") {
                slotX = x;
                slotY = y;
                slotW = remainW;
                slotH = child.DesiredSize.Height;
                y += slotH;
                remainH -= slotH;
            } else if (dock == "Bottom") {
                slotW = remainW;
                slotH = child.DesiredSize.Height;
                slotX = x;
                slotY = finalSize.Height - slotH;
                remainH -= slotH;
            } else if (dock == "Right") {
                slotH = remainH;
                slotW = child.DesiredSize.Width;
                slotX = finalSize.Width - slotW;
                slotY = y;
                remainW -= slotW;
            } else {
                slotX = x;
                slotY = y;
                slotW = child.DesiredSize.Width;
                slotH = remainH;
                x += slotW;
                remainW -= slotW;
            }
            LayoutHelper.ArrangeChild(panel, child, slotX, slotY, slotW, slotH);
        }
    }
}
