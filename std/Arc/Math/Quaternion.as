namespace Arc.Math;

/// <summary>
/// 四元数（X, Y, Z, W），值类型，用于 3D 旋转。
/// 对齐 C# System.Numerics 命名习惯，采用 double 精度；W 为实部。
/// </summary>
public struct Quaternion {
    /// <summary>X 分量（虚部）。</summary>
    public double X;
    /// <summary>Y 分量（虚部）。</summary>
    public double Y;
    /// <summary>Z 分量（虚部）。</summary>
    public double Z;
    /// <summary>W 分量（实部）。</summary>
    public double W;

    /// <summary>构造四元数。</summary>
    public Quaternion(double x, double y, double z, double w) {
        X = x;
        Y = y;
        Z = z;
        W = w;
    }

    /// <summary>单位四元数（无旋转）。</summary>
    public static readonly Quaternion Identity = new Quaternion(0.0, 0.0, 0.0, 1.0);

    /// <summary>四元数共轭。</summary>
    public Quaternion Conjugate() {
        return new Quaternion(-X, -Y, -Z, W);
    }

    /// <summary>四元数乘法（this * other）。</summary>
    public Quaternion Multiply(Quaternion other) {
        return new Quaternion(
            W * other.X + X * other.W + Y * other.Z - Z * other.Y,
            W * other.Y - X * other.Z + Y * other.W + Z * other.X,
            W * other.Z + X * other.Y - Y * other.X + Z * other.W,
            W * other.W - X * other.X - Y * other.Y - Z * other.Z);
    }

    /// <summary>四元数长度平方。</summary>
    public double LengthSquared() {
        return X * X + Y * Y + Z * Z + W * W;
    }

    /// <summary>归一化四元数（长度变为 1）。</summary>
    public Quaternion Normalize() {
        double len = Math.Sqrt(LengthSquared());
        if (len < 0.0001) {
            return new Quaternion(0.0, 0.0, 0.0, 1.0);
        }
        double inv = 1.0 / len;
        return new Quaternion(X * inv, Y * inv, Z * inv, W * inv);
    }

    /// <summary>绕任意轴旋转（轴需已归一化；角度弧度）。</summary>
    public static Quaternion CreateFromAxisAngle(Vector3 axis, double angle) {
        double half = angle * 0.5;
        double s = Math.Sin(half);
        return new Quaternion(axis.X * s, axis.Y * s, axis.Z * s, Math.Cos(half));
    }

    /// <summary>由 yaw/pitch/roll 欧拉角构造（弧度，顺序 Y→X→Z）。</summary>
    public static Quaternion CreateFromYawPitchRoll(double yaw, double pitch, double roll) {
        double hr = roll * 0.5;
        double hp = pitch * 0.5;
        double hy = yaw * 0.5;
        double sr = Math.Sin(hr);
        double cr = Math.Cos(hr);
        double sp = Math.Sin(hp);
        double cp = Math.Cos(hp);
        double sy = Math.Sin(hy);
        double cy = Math.Cos(hy);
        return new Quaternion(
            cy * sp * cr + sy * cp * sr,
            sy * cp * cr - cy * sp * sr,
            cy * cp * sr - sy * sp * cr,
            cy * cp * cr + sy * sp * sr);
    }
}