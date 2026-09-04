// RFC 037 D5.1: Arc.UI.Layout — Flexbox 一维布局（StackPanel）。
//
// 不使用 `is` 类型测试（MIR/PlatformTreeSync 同构约束）；UI 子节点直接 cast。

namespace Arc.UI.Layout;

using Arc.UI;

/// <summary>Flexbox 一维布局（StackPanel）。</summary>
internal class Flexbox {
    public static LayoutSize MeasureStack(Panel panel, Orientation orientation,
                                           double spacing, LayoutSize available) {
        if (panel == null || panel.Children == null) {
            return new LayoutSize(0.0, 0.0);
        }
        bool horizontal = orientation == Orientation.Horizontal;
        double maxCross = 0.0;
        double mainSum = 0.0;
        double availW = available.Width;
        double availH = available.Height;
        bool wBounded = availW > 0.0 && availW < 1000000000.0;
        bool hBounded = availH > 0.0 && availH < 1000000000.0;
        int count = panel.Children.Count;

        for (int i = 0; i < count; i++) {
            Element raw = panel.Children[i];
            FrameworkElement child = (FrameworkElement)raw;
            // 传递子约束：主轴方向无限（让子元素按自身大小测量），交叉轴方向使用可用约束
            if (horizontal) {
                // 水平方向：宽度无限（主轴），高度受约束（交叉轴）
                double childH = hBounded ? availH : LayoutHelper.Unbounded;
                LayoutHelper.MeasureChild(child, new LayoutSize(LayoutHelper.Unbounded, childH));
            } else {
                // 垂直方向：高度无限（主轴），宽度受约束（交叉轴）
                double childW = wBounded ? availW : LayoutHelper.Unbounded;
                LayoutHelper.MeasureChild(child, new LayoutSize(childW, LayoutHelper.Unbounded));
            }
            LayoutSize d = child.DesiredSize;
            if (horizontal) {
                mainSum += d.Width;
                int next = i + 1;
                if (next < count) {
                    mainSum += spacing;
                }
                if (d.Height > maxCross) {
                    maxCross = d.Height;
                }
            } else {
                mainSum += d.Height;
                int next = i + 1;
                if (next < count) {
                    mainSum += spacing;
                }
                if (d.Width > maxCross) {
                    maxCross = d.Width;
                }
            }
        }

        if (horizontal) {
            return new LayoutSize(mainSum, maxCross);
        }
        return new LayoutSize(maxCross, mainSum);
    }

    public static void ArrangeStack(Panel panel, Orientation orientation,
                                     double spacing, LayoutSize finalSize) {
        if (panel == null || panel.Children == null) {
            return;
        }
        bool horizontal = orientation == Orientation.Horizontal;
        double main = 0.0;
        int count = panel.Children.Count;

        for (int i = 0; i < count; i++) {
            Element raw = panel.Children[i];
            FrameworkElement child = (FrameworkElement)raw;
            LayoutSize d = child.DesiredSize;
            if (horizontal) {
                LayoutHelper.ArrangeChild(panel, child, main, 0.0, d.Width, finalSize.Height);
                main += d.Width + spacing;
            } else {
                LayoutHelper.ArrangeChild(panel, child, 0.0, main, finalSize.Width, d.Height);
                main += d.Height + spacing;
            }
        }
    }

    /// <summary>测量 WrapPanel（水平/垂直换行）。</summary>
    public static LayoutSize MeasureWrap(Panel panel, Orientation orientation,
                                         double itemW, double itemH,
                                         double spacing, LayoutSize available) {
        if (panel == null || panel.Children == null) {
            return new LayoutSize(0.0, 0.0);
        }
        bool horizontal = orientation != Orientation.Vertical;
        double availW = available.Width;
        double availH = available.Height;
        double lineMain = 0.0;
        double lineCross = 0.0;
        double totalCross = 0.0;
        double maxMain = 0.0;
        double limit = horizontal ? availW : availH;
        int count = panel.Children.Count;

        for (int i = 0; i < count; i++) {
            Element raw = panel.Children[i];
            FrameworkElement child = (FrameworkElement)raw;
            double childW = availW;
            double childH = availH;
            if (itemW > 0.0) {
                childW = itemW;
            }
            if (itemH > 0.0) {
                childH = itemH;
            }
            LayoutHelper.MeasureChild(child, new LayoutSize(childW, childH));
            LayoutSize d = child.DesiredSize;
            double cw = itemW > 0.0 ? itemW : d.Width;
            double ch = itemH > 0.0 ? itemH : d.Height;

            if (horizontal) {
                if (limit > 0.0 && lineMain > 0.0 && lineMain + cw > limit) {
                    totalCross += lineCross + spacing;
                    maxMain = LayoutHelper.Max(maxMain, lineMain);
                    lineMain = 0.0;
                    lineCross = 0.0;
                }
                lineMain += cw;
                if (i + 1 < count) {
                    lineMain += spacing;
                }
                if (ch > lineCross) {
                    lineCross = ch;
                }
            } else {
                if (limit > 0.0 && lineMain > 0.0 && lineMain + ch > limit) {
                    totalCross += lineCross + spacing;
                    maxMain = LayoutHelper.Max(maxMain, lineMain);
                    lineMain = 0.0;
                    lineCross = 0.0;
                }
                lineMain += ch;
                if (i + 1 < count) {
                    lineMain += spacing;
                }
                if (cw > lineCross) {
                    lineCross = cw;
                }
            }
        }
        totalCross += lineCross;
        maxMain = LayoutHelper.Max(maxMain, lineMain);
        if (horizontal) {
            return new LayoutSize(maxMain, totalCross);
        }
        return new LayoutSize(totalCross, maxMain);
    }

    /// <summary>排列 WrapPanel 子元素。</summary>
    public static void ArrangeWrap(Panel panel, Orientation orientation,
                                    double itemWidth, double itemHeight,
                                    double spacing, LayoutSize finalSize) {
        if (panel == null || panel.Children == null) {
            return;
        }
        bool horizontal = orientation != Orientation.Vertical;
        double lineMain = 0.0;
        double lineCross = 0.0;
        double limit = horizontal ? finalSize.Width : finalSize.Height;
        int count = panel.Children.Count;

        for (int i = 0; i < count; i++) {
            Element raw = panel.Children[i];
            FrameworkElement child = (FrameworkElement)raw;
            LayoutSize d = child.DesiredSize;
            double cw = itemWidth > 0.0 ? itemWidth : d.Width;
            double ch = itemHeight > 0.0 ? itemHeight : d.Height;

            if (horizontal) {
                if (limit > 0.0 && lineMain > 0.0 && lineMain + cw > limit) {
                    lineMain = 0.0;
                    lineCross += (itemHeight > 0.0 ? itemHeight : ch) + spacing;
                }
                double slotH = finalSize.Height;
                if (itemHeight > 0.0) {
                    slotH = itemHeight;
                }
                LayoutHelper.ArrangeChild(panel, child, lineMain, lineCross, cw, slotH);
                lineMain += cw + spacing;
            } else {
                if (limit > 0.0 && lineMain > 0.0 && lineMain + ch > limit) {
                    lineMain = 0.0;
                    lineCross += (itemWidth > 0.0 ? itemWidth : cw) + spacing;
                }
                double slotW = finalSize.Width;
                if (itemWidth > 0.0) {
                    slotW = itemWidth;
                }
                LayoutHelper.ArrangeChild(panel, child, lineCross, lineMain, slotW, ch);
                lineMain += ch + spacing;
            }
        }
    }
}
