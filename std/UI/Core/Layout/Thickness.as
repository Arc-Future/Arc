// RFC 037 D5.4: Arc.UI.Layout — Thickness 边距单位。
//
// Thickness 表示矩形的四边边距，可统一值或分别指定。
// L3 骨架诚实：纯值类型可证伪（ui_skeleton_honesty_e2e）；≠ 布局引擎已完成。

namespace Arc.UI.Layout;

using Arc;

/// <summary>矩形四边边距。</summary>
public struct Thickness {
    /// <summary>左边距。</summary>
    public double Left;

    /// <summary>上边距。</summary>
    public double Top;

    /// <summary>右边距。</summary>
    public double Right;

    /// <summary>下边距。</summary>
    public double Bottom;

    public Thickness() { }

    /// <summary>统一边距构造。</summary>
    /// <param name="uniform">四边统一值。</param>
    public Thickness(double uniform) {
        this.Left = uniform;
        this.Top = uniform;
        this.Right = uniform;
        this.Bottom = uniform;
    }

    /// <summary>水平/垂直边距构造。</summary>
    /// <param name="horizontal">左右边距。</param>
    /// <param name="vertical">上下边距。</param>
    public Thickness(double horizontal, double vertical) {
        this.Left = horizontal;
        this.Right = horizontal;
        this.Top = vertical;
        this.Bottom = vertical;
    }

    /// <summary>四边独立构造。</summary>
    public Thickness(double left, double top, double right, double bottom) {
        this.Left = left;
        this.Top = top;
        this.Right = right;
        this.Bottom = bottom;
    }

    public static Thickness Parse(string value) {
        if (value == null || value.Length == 0) {
            return new Thickness(0.0);
        }

        int comma1 = value.IndexOf(',');
        if (comma1 < 0) {
            double uniform = LayoutHelper.ParseNumber(value, 0.0);
            return new Thickness(uniform);
        }

        int comma2 = value.IndexOf(',', comma1 + 1);
        if (comma2 < 0) {
            double horizontal = LayoutHelper.ParseNumber(value.Substring(0, comma1), 0.0);
            double vertical = LayoutHelper.ParseNumber(value.Substring(comma1 + 1), 0.0);
            return new Thickness(horizontal, vertical);
        }

        int comma3 = value.IndexOf(',', comma2 + 1);
        if (comma3 < 0) {
            return new Thickness(0.0);
        }

        double left = LayoutHelper.ParseNumber(value.Substring(0, comma1), 0.0);
        double top = LayoutHelper.ParseNumber(value.Substring(comma1 + 1, comma2 - comma1 - 1), 0.0);
        double right = LayoutHelper.ParseNumber(value.Substring(comma2 + 1, comma3 - comma2 - 1), 0.0);
        double bottom = LayoutHelper.ParseNumber(value.Substring(comma3 + 1), 0.0);
        return new Thickness(left, top, right, bottom);
    }

    public Thickness Sanitized() {
        return new Thickness(
            LayoutHelper.Sanitize(this.Left),
            LayoutHelper.Sanitize(this.Top),
            LayoutHelper.Sanitize(this.Right),
            LayoutHelper.Sanitize(this.Bottom));
    }
}
