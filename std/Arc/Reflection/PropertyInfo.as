// RFC 018 §4.2.6: 属性信息——对齐 C# System.Reflection.PropertyInfo。
//
// **永久剔除 GetValue() / SetValue()**——元数据 vs 反射的物理边界（RFC 018 §3.2 / §3.3）。
// 属性反射调用（PropertyInfo.GetValue / PropertyInfo.SetValue）永久剔除，
// 与 1.4「拒绝运行时反射调用」裁决一致。

namespace Arc.Reflection;

/// <summary>
/// 属性信息——对齐 C# System.Reflection.PropertyInfo。
///
/// **永久剔除 GetValue() / SetValue()**——元数据 vs 反射的物理边界
/// （RFC 018 §3.2 / §3.3）。属性反射调用（PropertyInfo.GetValue /
/// PropertyInfo.SetValue）永久剔除，与 1.4「拒绝运行时反射调用」裁决一致。
/// </summary>
public abstract class PropertyInfo : MemberInfo {
    /// <summary>属性类型。</summary>
    /// <remarks>
    /// RFC 018 J-C：抽象属性——具体实现（RuntimePropertyInfo）由 codegen 从
    /// RtPropertyInfo.property_type 拦截填充；禁止带存储的自动属性，否则 `p.PropertyType`
    /// 会读零初始化字段而非元数据。
    /// </remarks>
    public abstract Type PropertyType { get; }

    /// <summary>属性是否可读（有 getter）。</summary>
    public bool CanRead { get; }

    /// <summary>属性是否可写（有 setter）。</summary>
    public bool CanWrite { get; }

    /// <summary>getter 方法信息（无可读时为 null）。</summary>
    public MethodInfo? GetMethod { get; }

    /// <summary>setter 方法信息（无可写时为 null）。</summary>
    public MethodInfo? SetMethod { get; }

    /// <summary>属性特性位掩码（C# System.Reflection.PropertyAttributes 对齐）。</summary>
    public PropertyAttributes Attributes { get; }

    /// <summary>受保护构造函数——具体 PropertyInfo 派生类通过 : base() 调用。</summary>
    protected PropertyInfo() {}
}
