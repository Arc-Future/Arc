// RFC 037 §3.2 + WPF 精华：Arc.UI.Layout — CornerRadius 圆角四角结构。
//
// 对标 System.Windows.CornerRadius：四角独立圆角半径（TL/TR/BR/BL），
// 可统一值或分别指定（WPF 同构）。作为控件圆角的结构化类型，编译期固化，
// 渲染器按四角落地圆角绘制（WgpuRender 圆角 SDF 填充/描边）。

namespace Arc.UI.Layout;

/// <summary>圆角四角结构（像素）。</summary>
public struct CornerRadius {
    /// <summary>左上角半径。</summary>
    public double TopLeft;

    /// <summary>右上角半径。</summary>
    public double TopRight;

    /// <summary>右下角半径。</summary>
    public double BottomRight;

    /// <summary>左下角半径。</summary>
    public double BottomLeft;

    public CornerRadius() {
    }

    /// <summary>四角统一半径。</summary>
    public CornerRadius(double uniform) {
        this.TopLeft = uniform;
        this.TopRight = uniform;
        this.BottomRight = uniform;
        this.BottomLeft = uniform;
    }

    /// <summary>对角线统一（左上/右下、右上/左下各一组）。</summary>
    public CornerRadius(double topLeftBottomRight, double topRightBottomLeft) {
        this.TopLeft = topLeftBottomRight;
        this.TopRight = topRightBottomLeft;
        this.BottomRight = topLeftBottomRight;
        this.BottomLeft = topRightBottomLeft;
    }

    /// <summary>四角分别指定。</summary>
    public CornerRadius(double topLeft, double topRight, double bottomRight, double bottomLeft) {
        this.TopLeft = topLeft;
        this.TopRight = topRight;
        this.BottomRight = bottomRight;
        this.BottomLeft = bottomLeft;
    }

    /// <summary>最大统一半径（用于阴影贴合等单值场景）。</summary>
    public double Max {
        get {
            double m = this.TopLeft;
            if (this.TopRight > m) { m = this.TopRight; }
            if (this.BottomRight > m) { m = this.BottomRight; }
            if (this.BottomLeft > m) { m = this.BottomLeft; }
            return m;
        }
    }
}