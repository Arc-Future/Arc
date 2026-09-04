namespace Arc.Illusory;

/// <summary>全局唯一、单调递增的 Actor 身份；确定性/回放/网络引用锚点。</summary>
/// <remarks>
/// 值语义，可作字典键；<see cref="None"/> 表示无效/未分配。实例分配与回收由
/// World 内部注册表负责，避免绕过生命周期约束。
/// </remarks>
public readonly struct ActorId : IEquatable<ActorId>, IHashable<ActorId> {
    private readonly int _value;

    /// <summary>无效/未分配的哨兵标识（== default）。</summary>
    public static readonly ActorId None;

    /// <summary>底层序号，从 1 起单调递增。</summary>
    public int Value {
        get { return _value; }
    }

    /// <summary>内部构造——仅供 World 注册表分配 Id。</summary>
    internal ActorId(int value) {
        _value = value;
    }

    public static bool Equals(ActorId a, ActorId b) {
        return a._value == b._value;
    }

    public static int GetHashCode(ActorId value) {
        return HashCode.HashValue(value._value);
    }
}