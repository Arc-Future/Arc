// RFC 018 §4.2.5: 方法信息——对齐 C# System.Reflection.MethodInfo。
//
// 在 MethodBase 基础上补充返回类型信息。
// **永久剔除 Invoke()**——元数据 vs 反射的物理边界（RFC 018 §3.2 / §3.3）。

namespace Arc.Reflection;

using Arc.Collections;

/// <summary>
/// 方法信息——对齐 C# System.Reflection.MethodInfo。
///
/// 在 MethodBase 基础上补充返回类型信息。
/// **永久剔除 Invoke()**——元数据 vs 反射的物理边界（RFC 018 §3.2 / §3.3）。
/// </summary>
public abstract class MethodInfo : MethodBase {
    /// <summary>方法返回类型。</summary>
    /// <remarks>
    /// RFC 018 J-C：抽象属性——具体实现（RuntimeMethodInfo）由 codegen 从
    /// RtMethodInfo.return_type 拦截填充；禁止带存储的自动属性，否则 `m.ReturnType`
    /// 会读零初始化字段而非元数据。
    /// </remarks>
    public abstract Type ReturnType { get; }

    /// <summary>受保护构造函数——具体 MethodInfo 派生类通过 : base() 调用。</summary>
    protected MethodInfo() {}

    /// <summary>返回方法形参列表（覆写 MethodBase）。</summary>
    /// <returns>形参信息列表。</returns>
    public override abstract List<ParameterInfo> GetParameters();
}
