// RFC 018 §4.2.6: 字段信息——对齐 C# System.Reflection.FieldInfo。
//
// **永久剔除 GetValue() / SetValue()**——元数据 vs 反射的物理边界（RFC 018 §3.2 / §3.3）。
// 字段反射调用（FieldInfo.GetValue / FieldInfo.SetValue）永久剔除，
// 与 1.4「拒绝运行时反射调用」裁决一致。

namespace Arc.Reflection;

/// <summary>
/// 字段信息——对齐 C# System.Reflection.FieldInfo。
///
/// **永久剔除 GetValue() / SetValue()**——元数据 vs 反射的物理边界
/// （RFC 018 §3.2 / §3.3）。字段反射调用（FieldInfo.GetValue / FieldInfo.SetValue）
/// 永久剔除，与 1.4「拒绝运行时反射调用」裁决一致。
/// </summary>
public abstract class FieldInfo : MemberInfo {
    /// <summary>字段类型。</summary>
    /// <remarks>
    /// RFC 018 J-C：抽象属性——具体实现（RuntimeFieldInfo）由 codegen 从
    /// RtFieldInfo.field_type 拦截填充；禁止带存储的自动属性，否则 `f.FieldType`
    /// 会读零初始化字段而非元数据。
    /// </remarks>
    public abstract Type FieldType { get; }

    /// <summary>是否为静态字段。</summary>
    public bool IsStatic { get; }

    /// <summary>是否为只读字段（readonly）。</summary>
    public bool IsInitOnly { get; }

    /// <summary>是否为常量字段（const）。</summary>
    public bool IsLiteral { get; }

    /// <summary>是否为 public 字段。</summary>
    public bool IsPublic { get; }

    /// <summary>是否为 private 字段。</summary>
    public bool IsPrivate { get; }

    /// <summary>字段特性位掩码（C# System.Reflection.FieldAttributes 对齐）。</summary>
    public FieldAttributes Attributes { get; }

    /// <summary>受保护构造函数——具体 FieldInfo 派生类通过 : base() 调用。</summary>
    protected FieldInfo() {}
}
