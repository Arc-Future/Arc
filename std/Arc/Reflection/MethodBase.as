// RFC 018 §4.2.5: 方法成员抽象基类——对齐 C# System.Reflection.MethodBase。
//
// MethodInfo 与 ConstructorInfo 的共同基类。
// **永久剔除 Invoke()**——这是元数据 vs 反射的物理边界（RFC 018 §3.2 / §3.3）。

namespace Arc.Reflection;

using Arc.Collections;

/// <summary>
/// 方法成员抽象基类——对齐 C# System.Reflection.MethodBase。
///
/// MethodInfo 与 ConstructorInfo 的共同基类。
/// **永久剔除 Invoke()**——这是元数据 vs 反射的物理边界（RFC 018 §3.2 / §3.3）。
/// </summary>
public abstract class MethodBase : MemberInfo {
    /// <summary>方法形参列表（顺序与声明一致）。</summary>
    /// <returns>形参信息列表。</returns>
    public abstract List<ParameterInfo> GetParameters();

    /// <summary>是否为静态方法。</summary>
    public bool IsStatic { get; }

    /// <summary>是否为抽象方法（abstract class 的 abstract 方法或接口方法）。</summary>
    public bool IsAbstract { get; }

    /// <summary>是否为虚方法（virtual 或 override）。</summary>
    public bool IsVirtual { get; }

    /// <summary>是否为 public 方法。</summary>
    public bool IsPublic { get; }

    /// <summary>是否为 private 方法。</summary>
    public bool IsPrivate { get; }

    /// <summary>方法特性位掩码（C# System.Reflection.MethodAttributes 对齐）。</summary>
    public MethodAttributes Attributes { get; }

    /// <summary>受保护构造函数——派生类（MethodInfo/ConstructorInfo）通过 : base() 调用。</summary>
    protected MethodBase() {}
}
