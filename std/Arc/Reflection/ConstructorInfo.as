// RFC 018 §4.2.5: 构造函数信息——对齐 C# System.Reflection.ConstructorInfo。
//
// **永久剔除 Invoke()**——元数据 vs 反射的物理边界（RFC 018 §3.2 / §3.3）。
// 构造函数的反射调用（Activator.CreateInstance / ConstructorInfo.Invoke）永久剔除，
// 与 1.4「拒绝运行时反射调用」裁决一致。

namespace Arc.Reflection;

using Arc.Collections;

/// <summary>
/// 构造函数信息——对齐 C# System.Reflection.ConstructorInfo。
///
/// **永久剔除 Invoke()**——元数据 vs 反射的物理边界（RFC 018 §3.2 / §3.3）。
/// 构造函数的反射调用（Activator.CreateInstance / ConstructorInfo.Invoke）永久剔除，
/// 与 1.4「拒绝运行时反射调用」裁决一致。
/// </summary>
public abstract class ConstructorInfo : MethodBase {
    /// <summary>受保护构造函数——具体 ConstructorInfo 派生类通过 : base() 调用。</summary>
    protected ConstructorInfo() {}

    /// <summary>返回构造函数形参列表（覆写 MethodBase）。</summary>
    /// <returns>形参信息列表。</returns>
    public override abstract List<ParameterInfo> GetParameters();
}
