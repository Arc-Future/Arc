namespace Arc.Math;

/// <summary>
/// 2D 矩形（X/Y 左上角 + 宽高），值类型，用于相机视口、精灵摆放、碰撞。
/// 采用 double 精度；边界为包含语义（X ≤ point ≤ X+Width）。
/// </summary>
public struct Rect {
    /// <summary>左上角 X。</summary>
    public double X;
    /// <summary>左上角 Y。</summary>
    public double Y;
    /// <summary>宽度。</summary>
    public double Width;
    /// <summary>高度。</summary>
    public double Height;

    /// <summary>构造矩形。</summary>
    public Rect(double x, double y, double width, double height) {
        X = x;
        Y = y;
        Width = width;
        Height = height;
    }

    /// <summary>右侧（X + Width）。</summary>
    public double Right { get { return X + Width; } }

    /// <summary>底部（Y + Height）。</summary>
    public double Bottom { get { return Y + Height; } }

    /// <summary>是否包含点（包含边界）。</summary>
    public bool Contains(double px, double py) {
        return px >= X && px <= X + Width && py >= Y && py <= Y + Height;
    }

    /// <summary>是否包含另一个矩形。</summary>
    public bool Contains(Rect other) {
        return other.X >= X && other.Right <= Right && other.Y >= Y && other.Bottom <= Bottom;
    }

    /// <summary>是否与另一矩形相交（含边界接触）。</summary>
    public bool Intersects(Rect other) {
        return X <= other.Right && other.X <= Right && Y <= other.Bottom && other.Y <= Bottom;
    }

    /// <summary>并集矩形（包含两矩形的最小外接矩形）。</summary>
    public Rect Union(Rect other) {
        double left = Math.Min(X, other.X);
        double top = Math.Min(Y, other.Y);
        double right = Math.Max(Right, other.Right);
        double bottom = Math.Max(Bottom, other.Bottom);
        return new Rect(left, top, right - left, bottom - top);
    }

    /// <summary>交集矩形（无交叠时返回空矩形）。</summary>
    public Rect Intersect(Rect other) {
        if (!Intersects(other)) {
            return new Rect(0.0, 0.0, 0.0, 0.0);
        }
        double left = Math.Max(X, other.X);
        double top = Math.Max(Y, other.Y);
        double right = Math.Min(Right, other.Right);
        double bottom = Math.Min(Bottom, other.Bottom);
        return new Rect(left, top, right - left, bottom - top);
    }
}