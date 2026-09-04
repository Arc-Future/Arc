namespace Arc.Math;

/// <summary>
/// 场景层级变换（位置/旋转/缩放），值类型。
/// 提供局部→世界（TRS 矩阵）与世界→局部（逆矩阵）变换。
/// </summary>
public struct Transform {
    /// <summary>局部位置。</summary>
    public Vector3 Position;
    /// <summary>局部旋转（四元数）。</summary>
    public Quaternion Rotation;
    /// <summary>局部缩放。</summary>
    public Vector3 Scale;

    /// <summary>构造变换（默认单位旋转、单位缩放）。</summary>
    public Transform(Vector3 position, Quaternion rotation, Vector3 scale) {
        Position = position;
        Rotation = rotation;
        Scale = scale;
    }

    /// <summary>单位变换（原点、无旋转、单位缩放）。</summary>
    public static readonly Transform Identity = new Transform(Vector3.Zero, Quaternion.Identity, Vector3.One);

    /// <summary>由旋转四元数构造旋转矩阵（已归一化）。</summary>
    public static Matrix4x4 RotationMatrix(Quaternion q) {
        double x = q.X;
        double y = q.Y;
        double z = q.Z;
        double w = q.W;
        return new Matrix4x4(
            1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y + w * z), 2.0 * (x * z - w * y), 0.0,
            2.0 * (x * y - w * z), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z + w * x), 0.0,
            2.0 * (x * z + w * y), 2.0 * (y * z - w * x), 1.0 - 2.0 * (x * x + y * y), 0.0,
            0.0, 0.0, 0.0, 1.0);
    }

    /// <summary>局部→世界矩阵（T * R * S）。</summary>
    public Matrix4x4 LocalToWorld() {
        Matrix4x4 t = Matrix4x4.CreateTranslation(Position.X, Position.Y, Position.Z);
        Matrix4x4 r = RotationMatrix(Rotation);
        Matrix4x4 s = Matrix4x4.CreateScale(Scale.X, Scale.Y, Scale.Z);
        return t.Multiply(r.Multiply(s));
    }

    /// <summary>世界→局部矩阵（局部→世界矩阵的逆）。</summary>
    public Matrix4x4 WorldToLocal() {
        return LocalToWorld().Invert();
    }

    /// <summary>局部点 → 世界点。</summary>
    public Vector3 TransformPoint(Vector3 local) {
        Vector4 w = LocalToWorld().Transform(new Vector4(local.X, local.Y, local.Z, 1.0));
        return new Vector3(w.X, w.Y, w.Z);
    }

    /// <summary>世界点 → 局部点。</summary>
    public Vector3 TransformPointInverse(Vector3 world) {
        Vector4 l = WorldToLocal().Transform(new Vector4(world.X, world.Y, world.Z, 1.0));
        return new Vector3(l.X, l.Y, l.Z);
    }
}