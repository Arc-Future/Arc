// RFC 018 §4.3: 类型特性位掩码——对齐 C# System.Reflection.TypeAttributes。
//
// **设计偏差（vs RFC 018 §4.3）**：
// RFC 原设计 `public enum TypeAttributes { Abstract = 0x0800, ... }` 需要枚举
// 显式值语法，但当前 Arc enum AST 不支持显式值（仅支持命名变体）。为保持位掩码
// 组合语义（`TypeAttributes.Abstract | TypeAttributes.Sealed`），改用
// `class + public const int` 常量实现。语义与 RFC 等价。未来 enum 显式值语法
// 落地后可回到 RFC 原设计。（与 std/Arc/Attribute.as 中 AttributeTargets 同模式。）

namespace Arc.Reflection;

/// <summary>
/// 类型特性位掩码——对齐 C# System.Reflection.TypeAttributes。
///
/// 设计偏差：RFC 018 §4.3 原设计为 enum + 显式值，当前 Arc enum AST 不支持
/// 显式值，故以 `class + public const int` 实现，保持位掩码组合语义。
/// 用法示例：`TypeAttributes.Abstract | TypeAttributes.Sealed`。
/// </summary>
public class TypeAttributes {
    /// <summary>抽象类型（abstract class 或 interface）。</summary>
    public const int Abstract  = 0x0800;
    /// <summary>sealed 类型（不可继承）。</summary>
    public const int Sealed    = 0x0040;
    /// <summary>接口类型。</summary>
    public const int Interface = 0x0020;
    /// <summary>公开类型（顶层可见）。</summary>
    public const int Public    = 0x0001;
    /// <summary>非公开类型（仅程序集内可见）。</summary>
    public const int NotPublic = 0x0002;
}
