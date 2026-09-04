// RFC 018 §4.3: 字段特性位掩码——对齐 C# System.Reflection.FieldAttributes。
//
// **设计偏差（vs RFC 018 §4.3）**：
// RFC 原设计 `public enum FieldAttributes { Public = 0x0001, ... }` 需要枚举
// 显式值语法，但当前 Arc enum AST 不支持显式值。改用 `class + public const int`
// 常量实现，保持位掩码组合语义。（与 std/Arc/Attribute.as 中 AttributeTargets 同模式。）

namespace Arc.Reflection;

/// <summary>
/// 字段特性位掩码——对齐 C# System.Reflection.FieldAttributes。
///
/// 设计偏差：RFC 018 §4.3 原设计为 enum + 显式值，当前 Arc enum AST 不支持
/// 显式值，故以 `class + public const int` 实现，保持位掩码组合语义。
/// 用法示例：`FieldAttributes.Public | FieldAttributes.Static`。
/// </summary>
public class FieldAttributes {
    /// <summary>公开字段。</summary>
    public const int Public   = 0x0001;
    /// <summary>私有字段。</summary>
    public const int Private  = 0x0002;
    /// <summary>静态字段。</summary>
    public const int Static   = 0x0010;
    /// <summary>只读字段（readonly / initonly）。</summary>
    public const int InitOnly = 0x0020;
    /// <summary>常量字段（const / literal）。</summary>
    public const int Literal  = 0x0040;
}
