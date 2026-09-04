namespace Arc.Math;

/// <summary>
/// 4D 向量（X, Y, Z, W），值类型，用于齐次坐标等。
/// 对齐 C# System.Numerics 命名习惯，采用 double 精度。
/// </summary>
public struct Vector4 {
    /// <summary>X 分量。</summary>
    public double X;
    /// <summary>Y 分量。</summary>
    public double Y;
    /// <summary>Z 分量。</summary>
    public double Z;
    /// <summary>W 分量。</summary>
    public double W;

    /// <summary>构造 4D 向量。</summary>
    public Vector4(double x, double y, double z, double w) {
        X = x;
        Y = y;
        Z = z;
        W = w;
    }

    /// <summary>零向量。</summary>
    public static readonly Vector4 Zero = new Vector4(0.0, 0.0, 0.0, 0.0);

    /// <summary>向量长度平方。</summary>
    public double LengthSquared() {
        return X * X + Y * Y + Z * Z + W * W;
    }

    /// <summary>向量长度。</summary>
    public double Length() {
        return Math.Sqrt(LengthSquared());
    }

    /// <summary>向量加法。</summary>
    public Vector4 Add(Vector4 other) {
        return new Vector4(X + other.X, Y + other.Y, Z + other.Z, W + other.W);
    }

    /// <summary>向量减法。</summary>
    public Vector4 Subtract(Vector4 other) {
        return new Vector4(X - other.X, Y - other.Y, Z - other.Z, W - other.W);
    }

    /// <summary>标量乘法。</summary>
    public Vector4 Multiply(double scalar) {
        return new Vector4(X * scalar, Y * scalar, Z * scalar, W * scalar);
    }

    /// <summary>点积。</summary>
    public double Dot(Vector4 other) {
        return X * other.X + Y * other.Y + Z * other.Z + W * other.W;
    }

    /// <summary>归一化向量（长度变为 1，方向不变）。</summary>
    public Vector4 Normalize() {
        double len = Length();
        if (len < 0.0001) {
            return new Vector4(0.0, 0.0, 0.0, 0.0);
        }
        return Multiply(1.0 / len);
    }
}