// RFC 018 §4.2.2 / §4.3: 成员特性位掩码——对齐 C# System.Reflection.MemberAttributes。
//
// **RFC 018 §4.3 遗漏补充**：
// RFC 018 §4.2.2 中 MemberInfo.Attributes 字段声明为 `public MemberAttributes Attributes { get; }`，
// 引用 MemberAttributes 类型；但 §4.3 枚举定义清单中遗漏了 MemberAttributes 枚举
// （仅列出 MemberTypes/TypeKind/TypeAttributes/MethodAttributes/FieldAttributes/
// PropertyAttributes/EventAttributes/ParameterAttributes/BindingFlags 九个）。
// 本文件为补充定义，遵循与其它 *Attributes 文件一致的 `class + public const int` 模式。
// RFC 018 后续修订应将此定义纳入 §4.3。
//
// **设计偏差（vs C# System.Reflection.MemberAttributes）**：
// C# 原设计为 enum + 显式值，当前 Arc enum AST 不支持显式值，故以
// `class + public const int` 实现，保持位掩码组合语义。
// （与 std/Arc/Attribute.as 中 AttributeTargets 同模式。）

namespace Arc.Reflection;

/// <summary>
/// 成员特性位掩码——对齐 C# System.Reflection.MemberAttributes。
///
/// 用于 MemberInfo.Attributes 字段，描述成员的访问级别与修饰符。
///
/// **RFC 018 §4.3 遗漏补充**：RFC 018 §4.2.2 引用此类型但 §4.3 未定义，
/// 本文件为补充实现，遵循与其它 *Attributes 文件一致的 class + const int 模式。
///
/// 设计偏差：C# 原设计为 enum + 显式值，当前 Arc enum AST 不支持显式值，
/// 故以 `class + public const int` 实现，保持位掩码组合语义。
/// 用法示例：`MemberAttributes.Public | MemberAttributes.Static`。
/// </summary>
public class MemberAttributes {
    /// <summary>访问级别掩码（与 Public/Private/Family/Assembly/FamORAssem 组合使用）。</summary>
    public const int AccessMask   = 0x0007;
    /// <summary>私有成员（仅声明类型内可见）。</summary>
    public const int Private      = 0x0001;
    /// <summary>程序集内可见（internal）。</summary>
    public const int Assembly     = 0x0002;
    /// <summary>家族可见（protected）。</summary>
    public const int Family       = 0x0004;
    /// <summary>家族或程序集可见（protected internal）。</summary>
    public const int FamORAssem   = 0x0005;
    /// <summary>公开成员（public）。</summary>
    public const int Public       = 0x0006;
    /// <summary>静态成员。</summary>
    public const int Static       = 0x0010;
    /// <summary>final 成员（不可被子类覆写）。</summary>
    public const int Final        = 0x0020;
    /// <summary>虚成员（virtual 或 override）。</summary>
    public const int Virtual      = 0x0080;
    /// <summary>抽象成员（abstract class 的 abstract 方法或接口方法）。</summary>
    public const int Abstract     = 0x0400;
    /// <summary>特殊名称成员（编译器生成的属性 getter/setter、事件 add/remove 等）。</summary>
    public const int SpecialName  = 0x0800;
}
