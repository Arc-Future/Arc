// RFC 037 D5.2: Arc.UI.Layout — GridLayout 网格布局算法（layout-v2）。
//
// ColumnDefinitions/RowDefinitions 支持 Auto / Star(*) / Pixel。
// 子元素 LayoutX/Y + RenderSize 由 LayoutHelper.ArrangeChild 写入。

namespace Arc.UI.Layout;

using Arc.Collections;
using Arc.UI;
using Arc.UI.Components.Layout;

/// <summary>二维网格布局（Grid 面板）。</summary>
internal class GridLayout {
    private static int rowIndex(Element elem) {
        int i = Grid.GetRow(elem);
        if (i < 0) {
            return 0;
        }
        return i;
    }

    private static int colIndex(Element elem) {
        int i = Grid.GetColumn(elem);
        if (i < 0) {
            return 0;
        }
        return i;
    }

    private static void bounds(Panel panel, out int maxRow, out int maxCol) {
        maxRow = 0;
        maxCol = 0;
        if (panel == null || panel.Children == null) {
            return;
        }
        int count = panel.Children.Count;
        for (int i = 0; i < count; i++) {
            Element raw = panel.Children[i];
            if (raw.TypeName == "ColumnDefinitions" || raw.TypeName == "RowDefinitions") {
                continue;
            }
            int row = rowIndex(raw);
            int col = colIndex(raw);
            if (row > maxRow) { maxRow = row; }
            if (col > maxCol) { maxCol = col; }
        }
    }

    private static int resolveColCount(Grid grid, int maxCol) {
        int fromChildren = maxCol + 1;
        if (grid.ColumnDefinitions == null) {
            return fromChildren;
        }
        int defCount = grid.ColumnDefinitions.Count;
        if (defCount > fromChildren) {
            return defCount;
        }
        return fromChildren;
    }

    private static int resolveRowCount(Grid grid, int maxRow) {
        int fromChildren = maxRow + 1;
        if (grid.RowDefinitions == null) {
            return fromChildren;
        }
        int defCount = grid.RowDefinitions.Count;
        if (defCount > fromChildren) {
            return defCount;
        }
        return fromChildren;
    }

    private static GridLength colLength(Grid grid, int index) {
        if (grid.ColumnDefinitions != null && index < grid.ColumnDefinitions.Count) {
            ColumnDefinition def = (ColumnDefinition)grid.ColumnDefinitions[index];
            return def.Width;
        }
        return GridLength.Star(1.0);
    }

    private static GridLength rowLength(Grid grid, int index) {
        if (grid.RowDefinitions != null && index < grid.RowDefinitions.Count) {
            RowDefinition def = (RowDefinition)grid.RowDefinitions[index];
            return def.Height;
        }
        return GridLength.Star(1.0);
    }

    private static bool isLayoutChild(Element raw) {
        if (raw == null) {
            return false;
        }
        string tn = raw.TypeName;
        if (tn == "ColumnDefinitions" || tn == "RowDefinitions") {
            return false;
        }
        return true;
    }

    private static void measureTracks(Grid grid, LayoutSize available,
                                      int colCount, int rowCount,
                                      List<double> colWidths, List<double> rowHeights) {
        double colSpacing = grid.ColumnSpacing;
        double rowSpacing = grid.RowSpacing;
        int ci = 0;
        while (ci < colCount) {
            colWidths.Add(0.0);
            ci++;
        }
        int ri = 0;
        while (ri < rowCount) {
            rowHeights.Add(0.0);
            ri++;
        }

        double starColWeight = 0.0;
        double starRowWeight = 0.0;
        ci = 0;
        while (ci < colCount) {
            GridLength gl = colLength(grid, ci);
            if (gl.UnitType == GridLength.UnitPixel) {
                colWidths[ci] = LayoutHelper.Sanitize(gl.Value);
            } else if (gl.UnitType == GridLength.UnitStar) {
                starColWeight += gl.Value;
            }
            ci++;
        }
        ri = 0;
        while (ri < rowCount) {
            GridLength gl = rowLength(grid, ri);
            if (gl.UnitType == GridLength.UnitPixel) {
                rowHeights[ri] = LayoutHelper.Sanitize(gl.Value);
            } else if (gl.UnitType == GridLength.UnitStar) {
                starRowWeight += gl.Value;
            }
            ri++;
        }

        if (grid.Children == null) {
            distributeStar(colCount, rowCount, colWidths, rowHeights,
                starColWeight, starRowWeight,
                available.Width, available.Height,
                colSpacing, rowSpacing, grid);
            return;
        }

        int count = grid.Children.Count;
        for (int i = 0; i < count; i++) {
            Element raw = grid.Children[i];
            if (!isLayoutChild(raw)) {
                continue;
            }
            FrameworkElement child = (FrameworkElement)raw;
            int col = colIndex(raw);
            int row = rowIndex(raw);
            if (col >= colCount) { col = colCount - 1; }
            if (row >= rowCount) { row = rowCount - 1; }
            double cw = LayoutHelper.Unbounded;
            GridLength cgl = colLength(grid, col);
            if (cgl.UnitType == GridLength.UnitPixel) {
                cw = colWidths[col];
            }
            double rh = LayoutHelper.Unbounded;
            GridLength rgl = rowLength(grid, row);
            if (rgl.UnitType == GridLength.UnitPixel) {
                rh = rowHeights[row];
            }
            LayoutHelper.MeasureChild(child, new LayoutSize(cw, rh));
            LayoutSize d = child.DesiredSize;
            if (cgl.UnitType == GridLength.UnitAuto && d.Width > colWidths[col]) {
                colWidths[col] = d.Width;
            }
            if (rgl.UnitType == GridLength.UnitAuto && d.Height > rowHeights[row]) {
                rowHeights[row] = d.Height;
            }
        }

        distributeStar(colCount, rowCount, colWidths, rowHeights,
            starColWeight, starRowWeight,
            available.Width, available.Height,
            colSpacing, rowSpacing, grid);
    }

    private static void distributeStar(int colCount, int rowCount,
                                       List<double> colWidths, List<double> rowHeights,
                                       double starColWeight, double starRowWeight,
                                       double availW, double availH,
                                       double colSpacing, double rowSpacing,
                                       Grid grid) {
        double fixedW = 0.0;
        int ci = 0;
        while (ci < colCount) {
            fixedW += colWidths[ci];
            if (ci + 1 < colCount) {
                fixedW += colSpacing;
            }
            ci++;
        }
        double fixedH = 0.0;
        int ri = 0;
        while (ri < rowCount) {
            fixedH += rowHeights[ri];
            if (ri + 1 < rowCount) {
                fixedH += rowSpacing;
            }
            ri++;
        }

        double remainW = availW - fixedW;
        if (remainW < 0.0) { remainW = 0.0; }
        if (starColWeight > 0.0 && remainW > 0.0) {
            ci = 0;
            while (ci < colCount) {
                GridLength gl = colLength(grid, ci);
                if (gl.UnitType == GridLength.UnitStar) {
                    colWidths[ci] = remainW * gl.Value / starColWeight;
                }
                ci++;
            }
        }

        double remainH = availH - fixedH;
        if (remainH < 0.0) { remainH = 0.0; }
        if (starRowWeight > 0.0 && remainH > 0.0) {
            ri = 0;
            while (ri < rowCount) {
                GridLength gl = rowLength(grid, ri);
                if (gl.UnitType == GridLength.UnitStar) {
                    rowHeights[ri] = remainH * gl.Value / starRowWeight;
                }
                ri++;
            }
        }
    }

    private static double sumTracks(List<double> sizes, double spacing) {
        double total = 0.0;
        int n = sizes.Count;
        int i = 0;
        while (i < n) {
            total += sizes[i];
            if (i + 1 < n) {
                total += spacing;
            }
            i++;
        }
        return total;
    }

    public static LayoutSize Measure(Grid panel, LayoutSize available) {
        if (panel == null) {
            return new LayoutSize(0.0, 0.0);
        }
        int maxRow = 0;
        int maxCol = 0;
        bounds(panel, out maxRow, out maxCol);
        int colCount = resolveColCount(panel, maxCol);
        int rowCount = resolveRowCount(panel, maxRow);
        if (colCount <= 0) { colCount = 1; }
        if (rowCount <= 0) { rowCount = 1; }

        List<double> colWidths = new List<double>();
        List<double> rowHeights = new List<double>();
        measureTracks(panel, available, colCount, rowCount, colWidths, rowHeights);

        double totalW = sumTracks(colWidths, panel.ColumnSpacing);
        double totalH = sumTracks(rowHeights, panel.RowSpacing);
        return new LayoutSize(totalW, totalH);
    }

    public static void Arrange(Grid panel, LayoutSize finalSize) {
        if (panel == null) {
            return;
        }
        if (panel.Children == null) {
            return;
        }

        int maxRow = 0;
        int maxCol = 0;
        bounds(panel, out maxRow, out maxCol);
        int colCount = resolveColCount(panel, maxCol);
        int rowCount = resolveRowCount(panel, maxRow);
        if (colCount <= 0) { colCount = 1; }
        if (rowCount <= 0) { rowCount = 1; }

        List<double> colWidths = new List<double>();
        List<double> rowHeights = new List<double>();
        measureTracks(panel, finalSize, colCount, rowCount, colWidths, rowHeights);

        double colSpacing = panel.ColumnSpacing;
        double rowSpacing = panel.RowSpacing;
        List<double> colOffsets = new List<double>();
        List<double> rowOffsets = new List<double>();
        double x = 0.0;
        int ci = 0;
        while (ci < colCount) {
            colOffsets.Add(x);
            x += colWidths[ci] + colSpacing;
            ci++;
        }
        double y = 0.0;
        int ri = 0;
        while (ri < rowCount) {
            rowOffsets.Add(y);
            y += rowHeights[ri] + rowSpacing;
            ri++;
        }

        int count = panel.Children.Count;
        for (int i = 0; i < count; i++) {
            Element raw = panel.Children[i];
            if (!isLayoutChild(raw)) {
                continue;
            }
            FrameworkElement child = (FrameworkElement)raw;
            int row = rowIndex(raw);
            int col = colIndex(raw);
            if (col >= colCount) { col = colCount - 1; }
            if (row >= rowCount) { row = rowCount - 1; }
            double cx = colOffsets[col];
            double cy = rowOffsets[row];
            double cw = colWidths[col];
            double ch = rowHeights[row];
            LayoutHelper.ArrangeChild(panel, child, cx, cy, cw, ch);
        }
    }
}
