namespace Arc.Math;

/// <summary>
/// 2D 向量（X, Y），值类型，用于位置、方向、纹理坐标等。
/// 对齐 C# System.Numerics 命名习惯，采用 double 精度。
/// </summary>
public struct Vector2 {
    /// <summary>X 分量。</summary>
    public double X;
    /// <summary>Y 分量。</summary>
    public double Y;

    /// <summary>构造 2D 向量。</summary>
    public Vector2(double x, double y) {
        X = x;
        Y = y;
    }

    /// <summary>零向量。</summary>
    public static readonly Vector2 Zero = new Vector2(0.0, 0.0);

    /// <summary>(1, 1)。</summary>
    public static readonly Vector2 One = new Vector2(1.0, 1.0);

    /// <summary>(1, 0) 单位 X。</summary>
    public static readonly Vector2 UnitX = new Vector2(1.0, 0.0);

    /// <summary>(0, 1) 单位 Y。</summary>
    public static readonly Vector2 UnitY = new Vector2(0.0, 1.0);

    /// <summary>向量长度平方。</summary>
    public double LengthSquared() {
        return X * X + Y * Y;
    }

    /// <summary>向量长度。</summary>
    public double Length() {
        return Math.Sqrt(LengthSquared());
    }

    /// <summary>向量加法。</summary>
    public Vector2 Add(Vector2 other) {
        return new Vector2(X + other.X, Y + other.Y);
    }

    /// <summary>向量减法。</summary>
    public Vector2 Subtract(Vector2 other) {
        return new Vector2(X - other.X, Y - other.Y);
    }

    /// <summary>标量乘法。</summary>
    public Vector2 Multiply(double scalar) {
        return new Vector2(X * scalar, Y * scalar);
    }

    /// <summary>点积。</summary>
    public double Dot(Vector2 other) {
        return X * other.X + Y * other.Y;
    }

    /// <summary>归一化向量（长度变为 1，方向不变）。</summary>
    public Vector2 Normalize() {
        double len = Length();
        if (len < 0.0001) {
            return new Vector2(0.0, 0.0);
        }
        return Multiply(1.0 / len);
    }

    /// <summary>两点欧几里得距离。</summary>
    public static double Distance(Vector2 a, Vector2 b) {
        return a.Subtract(b).Length();
    }
}