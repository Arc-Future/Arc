namespace Arc.Math;

/// <summary>
/// 3D 向量（X, Y, Z），值类型，用于 3D 位置、方向、法线等。
/// 对齐 C# System.Numerics 命名习惯，采用 double 精度。
/// </summary>
public struct Vector3 {
    /// <summary>X 分量。</summary>
    public double X;
    /// <summary>Y 分量。</summary>
    public double Y;
    /// <summary>Z 分量。</summary>
    public double Z;

    /// <summary>构造 3D 向量。</summary>
    public Vector3(double x, double y, double z) {
        X = x;
        Y = y;
        Z = z;
    }

    /// <summary>零向量。</summary>
    public static readonly Vector3 Zero = new Vector3(0.0, 0.0, 0.0);

    /// <summary>(1, 1, 1)。</summary>
    public static readonly Vector3 One = new Vector3(1.0, 1.0, 1.0);

    /// <summary>(1, 0, 0) 单位 X。</summary>
    public static readonly Vector3 UnitX = new Vector3(1.0, 0.0, 0.0);

    /// <summary>(0, 1, 0) 单位 Y。</summary>
    public static readonly Vector3 UnitY = new Vector3(0.0, 1.0, 0.0);

    /// <summary>(0, 0, 1) 单位 Z。</summary>
    public static readonly Vector3 UnitZ = new Vector3(0.0, 0.0, 1.0);

    /// <summary>向量长度平方。</summary>
    public double LengthSquared() {
        return X * X + Y * Y + Z * Z;
    }

    /// <summary>向量长度。</summary>
    public double Length() {
        return Math.Sqrt(LengthSquared());
    }

    /// <summary>向量加法。</summary>
    public Vector3 Add(Vector3 other) {
        return new Vector3(X + other.X, Y + other.Y, Z + other.Z);
    }

    /// <summary>向量减法。</summary>
    public Vector3 Subtract(Vector3 other) {
        return new Vector3(X - other.X, Y - other.Y, Z - other.Z);
    }

    /// <summary>标量乘法。</summary>
    public Vector3 Multiply(double scalar) {
        return new Vector3(X * scalar, Y * scalar, Z * scalar);
    }

    /// <summary>点积。</summary>
    public double Dot(Vector3 other) {
        return X * other.X + Y * other.Y + Z * other.Z;
    }

    /// <summary>叉积（右手系）。</summary>
    public Vector3 Cross(Vector3 other) {
        return new Vector3(
            Y * other.Z - Z * other.Y,
            Z * other.X - X * other.Z,
            X * other.Y - Y * other.X);
    }

    /// <summary>归一化向量（长度变为 1，方向不变）。</summary>
    public Vector3 Normalize() {
        double len = Length();
        if (len < 0.0001) {
            return new Vector3(0.0, 0.0, 0.0);
        }
        return Multiply(1.0 / len);
    }

    /// <summary>两点欧几里得距离。</summary>
    public static double Distance(Vector3 a, Vector3 b) {
        return a.Subtract(b).Length();
    }
}