namespace Arc.Math;

/// <summary>
/// 4x4 矩阵，值类型，用于 3D 投影/视图/模型变换。
/// 行主序存储（M11..M44），对齐 C# System.Numerics 命名习惯；
/// 采用 double 精度，Arc 语言内标量路径实现（不引入新 ABI）。
/// </summary>
public struct Matrix4x4 {
    /// <summary>第 1 行第 1 列。</summary>
    public double M11;
    /// <summary>第 1 行第 2 列。</summary>
    public double M12;
    /// <summary>第 1 行第 3 列。</summary>
    public double M13;
    /// <summary>第 1 行第 4 列。</summary>
    public double M14;
    /// <summary>第 2 行第 1 列。</summary>
    public double M21;
    /// <summary>第 2 行第 2 列。</summary>
    public double M22;
    /// <summary>第 2 行第 3 列。</summary>
    public double M23;
    /// <summary>第 2 行第 4 列。</summary>
    public double M24;
    /// <summary>第 3 行第 1 列。</summary>
    public double M31;
    /// <summary>第 3 行第 2 列。</summary>
    public double M32;
    /// <summary>第 3 行第 3 列。</summary>
    public double M33;
    /// <summary>第 3 行第 4 列。</summary>
    public double M34;
    /// <summary>第 4 行第 1 列。</summary>
    public double M41;
    /// <summary>第 4 行第 2 列。</summary>
    public double M42;
    /// <summary>第 4 行第 3 列。</summary>
    public double M43;
    /// <summary>第 4 行第 4 列。</summary>
    public double M44;

    /// <summary>构造 4x4 矩阵（16 元素，行主序）。</summary>
    public Matrix4x4(double m11, double m12, double m13, double m14,
                     double m21, double m22, double m23, double m24,
                     double m31, double m32, double m33, double m34,
                     double m41, double m42, double m43, double m44) {
        M11 = m11; M12 = m12; M13 = m13; M14 = m14;
        M21 = m21; M22 = m22; M23 = m23; M24 = m24;
        M31 = m31; M32 = m32; M33 = m33; M34 = m34;
        M41 = m41; M42 = m42; M43 = m43; M44 = m44;
    }

    /// <summary>单位矩阵。</summary>
    public static readonly Matrix4x4 Identity = new Matrix4x4(
        1.0, 0.0, 0.0, 0.0,
        0.0, 1.0, 0.0, 0.0,
        0.0, 0.0, 1.0, 0.0,
        0.0, 0.0, 0.0, 1.0);

    /// <summary>构造单位矩阵。</summary>
    public static Matrix4x4 CreateIdentity() {
        return new Matrix4x4(
            1.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            0.0, 0.0, 0.0, 1.0);
    }

    /// <summary>矩阵乘法（this * other）。</summary>
    public Matrix4x4 Multiply(Matrix4x4 other) {
        return new Matrix4x4(
            M11 * other.M11 + M12 * other.M21 + M13 * other.M31 + M14 * other.M41,
            M11 * other.M12 + M12 * other.M22 + M13 * other.M32 + M14 * other.M42,
            M11 * other.M13 + M12 * other.M23 + M13 * other.M33 + M14 * other.M43,
            M11 * other.M14 + M12 * other.M24 + M13 * other.M34 + M14 * other.M44,
            M21 * other.M11 + M22 * other.M21 + M23 * other.M31 + M24 * other.M41,
            M21 * other.M12 + M22 * other.M22 + M23 * other.M32 + M24 * other.M42,
            M21 * other.M13 + M22 * other.M23 + M23 * other.M33 + M24 * other.M43,
            M21 * other.M14 + M22 * other.M24 + M23 * other.M34 + M24 * other.M44,
            M31 * other.M11 + M32 * other.M21 + M33 * other.M31 + M34 * other.M41,
            M31 * other.M12 + M32 * other.M22 + M33 * other.M32 + M34 * other.M42,
            M31 * other.M13 + M32 * other.M23 + M33 * other.M33 + M34 * other.M43,
            M31 * other.M14 + M32 * other.M24 + M33 * other.M34 + M34 * other.M44,
            M41 * other.M11 + M42 * other.M21 + M43 * other.M31 + M44 * other.M41,
            M41 * other.M12 + M42 * other.M22 + M43 * other.M32 + M44 * other.M42,
            M41 * other.M13 + M42 * other.M23 + M43 * other.M33 + M44 * other.M43,
            M41 * other.M14 + M42 * other.M24 + M43 * other.M34 + M44 * other.M44);
    }

    /// <summary>变换 4D 向量（行向量 * 矩阵，等价列向量 M * v）。</summary>
    public Vector4 Transform(Vector4 v) {
        return new Vector4(
            M11 * v.X + M21 * v.Y + M31 * v.Z + M41 * v.W,
            M12 * v.X + M22 * v.Y + M32 * v.Z + M42 * v.W,
            M13 * v.X + M23 * v.Y + M33 * v.Z + M43 * v.W,
            M14 * v.X + M24 * v.Y + M34 * v.Z + M44 * v.W);
    }

    /// <summary>平移矩阵。</summary>
    public static Matrix4x4 CreateTranslation(double x, double y, double z) {
        return new Matrix4x4(
            1.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            x, y, z, 1.0);
    }

    /// <summary>缩放矩阵。</summary>
    public static Matrix4x4 CreateScale(double x, double y, double z) {
        return new Matrix4x4(
            x, 0.0, 0.0, 0.0,
            0.0, y, 0.0, 0.0,
            0.0, 0.0, z, 0.0,
            0.0, 0.0, 0.0, 1.0);
    }

    /// <summary>绕 X 轴旋转矩阵（角度弧度）。</summary>
    public static Matrix4x4 CreateRotationX(double radians) {
        double c = Math.Cos(radians);
        double s = Math.Sin(radians);
        return new Matrix4x4(
            1.0, 0.0, 0.0, 0.0,
            0.0, c, s, 0.0,
            0.0, -s, c, 0.0,
            0.0, 0.0, 0.0, 1.0);
    }

    /// <summary>绕 Y 轴旋转矩阵（角度弧度）。</summary>
    public static Matrix4x4 CreateRotationY(double radians) {
        double c = Math.Cos(radians);
        double s = Math.Sin(radians);
        return new Matrix4x4(
            c, 0.0, -s, 0.0,
            0.0, 1.0, 0.0, 0.0,
            s, 0.0, c, 0.0,
            0.0, 0.0, 0.0, 1.0);
    }

    /// <summary>绕 Z 轴旋转矩阵（角度弧度）。</summary>
    public static Matrix4x4 CreateRotationZ(double radians) {
        double c = Math.Cos(radians);
        double s = Math.Sin(radians);
        return new Matrix4x4(
            c, s, 0.0, 0.0,
            -s, c, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            0.0, 0.0, 0.0, 1.0);
    }

    /// <summary>正交投影矩阵（右手系，远裁剪面向 -Z）。</summary>
    public static Matrix4x4 CreateOrthographic(double width, double height, double zNear, double zFar) {
        double range = zFar - zNear;
        return new Matrix4x4(
            2.0 / width, 0.0, 0.0, 0.0,
            0.0, 2.0 / height, 0.0, 0.0,
            0.0, 0.0, -2.0 / range, 0.0,
            0.0, 0.0, -((zFar + zNear) / range), 1.0);
    }

    /// <summary>透视投影矩阵（右手系，垂直视场角，远裁剪面向 -Z）。</summary>
    public static Matrix4x4 CreatePerspective(double fieldOfViewRadians, double aspectRatio, double zNear, double zFar) {
        double h = 1.0 / Math.Tan(fieldOfViewRadians / 2.0);
        double w = h / aspectRatio;
        double range = zNear - zFar;
        return new Matrix4x4(
            w, 0.0, 0.0, 0.0,
            0.0, h, 0.0, 0.0,
            0.0, 0.0, (zFar + zNear) / range, -1.0,
            0.0, 0.0, 2.0 * zFar * zNear / range, 0.0);
    }

    /// <summary>矩阵求逆（伴随矩阵法；行列式为 0 时返回单位矩阵）。</summary>
    public Matrix4x4 Invert() {
        double a = M11 * M22 * M33 * M44 + M11 * M23 * M34 * M42 + M11 * M24 * M32 * M43
                 + M12 * M21 * M34 * M43 + M12 * M23 * M31 * M44 + M12 * M24 * M33 * M41
                 + M13 * M21 * M32 * M44 + M13 * M22 * M34 * M41 + M13 * M24 * M31 * M42
                 + M14 * M21 * M33 * M42 + M14 * M22 * M31 * M43 + M14 * M23 * M32 * M41;
        double b = M11 * M22 * M34 * M43 + M11 * M23 * M31 * M44 + M11 * M24 * M33 * M42
                 + M12 * M21 * M33 * M44 + M12 * M23 * M34 * M41 + M12 * M24 * M31 * M43
                 + M13 * M21 * M34 * M42 + M13 * M22 * M31 * M44 + M13 * M24 * M32 * M41
                 + M14 * M21 * M32 * M43 + M14 * M22 * M33 * M41 + M14 * M23 * M31 * M42;
        double c = M11 * M22 * M33 * M44 + M11 * M23 * M31 * M42 + M11 * M24 * M32 * M43
                 + M12 * M21 * M34 * M43 + M12 * M23 * M32 * M41 + M12 * M24 * M31 * M44
                 + M13 * M21 * M32 * M44 + M13 * M22 * M34 * M41 + M13 * M24 * M33 * M41
                 + M14 * M21 * M33 * M42 + M14 * M22 * M31 * M43 + M14 * M23 * M32 * M41;
        double d = M11 * M22 * M34 * M41 + M11 * M23 * M31 * M42 + M11 * M24 * M33 * M42
                 + M12 * M21 * M33 * M44 + M12 * M23 * M34 * M41 + M12 * M24 * M31 * M43
                 + M13 * M21 * M34 * M42 + M13 * M22 * M31 * M44 + M13 * M24 * M32 * M41
                 + M14 * M21 * M32 * M43 + M14 * M22 * M33 * M41 + M14 * M23 * M31 * M42;
        double det = a - b - c + d;
        if (det > -0.0001 && det < 0.0001) {
            return CreateIdentity();
        }
        double invDet = 1.0 / det;
        return new Matrix4x4(
            (M22 * M33 * M44 + M23 * M34 * M42 + M24 * M32 * M43
             - M22 * M34 * M43 - M23 * M32 * M44 - M24 * M33 * M42) * invDet,
            (M12 * M34 * M43 + M13 * M32 * M44 + M14 * M33 * M42
             - M12 * M33 * M44 - M13 * M34 * M42 - M14 * M32 * M43) * invDet,
            (M12 * M23 * M44 + M13 * M24 * M42 + M14 * M22 * M43
             - M12 * M24 * M43 - M13 * M22 * M44 - M14 * M23 * M42) * invDet,
            (M12 * M24 * M33 + M13 * M22 * M34 + M14 * M23 * M32
             - M12 * M23 * M34 - M13 * M24 * M32 - M14 * M22 * M33) * invDet,
            (M21 * M34 * M43 + M23 * M31 * M44 + M24 * M33 * M41
             - M21 * M33 * M44 - M23 * M34 * M41 - M24 * M31 * M43) * invDet,
            (M11 * M33 * M44 + M13 * M34 * M41 + M14 * M31 * M43
             - M11 * M34 * M43 - M13 * M31 * M44 - M14 * M33 * M41) * invDet,
            (M11 * M24 * M43 + M12 * M31 * M44 + M14 * M23 * M41
             - M11 * M23 * M44 - M12 * M24 * M41 - M14 * M21 * M43) * invDet,
            (M11 * M23 * M34 + M12 * M24 * M31 + M13 * M21 * M44
             - M11 * M24 * M33 - M12 * M21 * M34 - M13 * M23 * M41) * invDet,
            (M21 * M32 * M44 + M22 * M34 * M41 + M24 * M31 * M42
             - M21 * M34 * M42 - M22 * M31 * M44 - M24 * M32 * M41) * invDet,
            (M11 * M34 * M42 + M12 * M31 * M44 + M14 * M32 * M41
             - M11 * M32 * M44 - M12 * M34 * M41 - M14 * M31 * M42) * invDet,
            (M11 * M22 * M44 + M12 * M24 * M41 + M14 * M21 * M42
             - M11 * M24 * M42 - M12 * M21 * M44 - M14 * M22 * M41) * invDet,
            (M11 * M24 * M32 + M12 * M21 * M34 + M13 * M22 * M41
             - M11 * M22 * M34 - M12 * M24 * M31 - M13 * M21 * M42) * invDet,
            (M21 * M33 * M42 + M22 * M31 * M43 + M23 * M32 * M41
             - M21 * M32 * M43 - M22 * M33 * M41 - M23 * M31 * M42) * invDet,
            (M11 * M32 * M43 + M12 * M33 * M41 + M13 * M31 * M42
             - M11 * M33 * M42 - M12 * M31 * M43 - M13 * M32 * M41) * invDet,
            (M11 * M23 * M41 + M12 * M21 * M43 + M13 * M22 * M41
             - M11 * M22 * M43 - M12 * M23 * M41 - M13 * M21 * M42) * invDet,
            (M11 * M22 * M33 + M12 * M23 * M31 + M13 * M21 * M32
             - M11 * M23 * M32 - M12 * M21 * M33 - M13 * M22 * M31) * invDet);
    }
}