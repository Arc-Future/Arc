// RFC 018 §4.3: 属性特性位掩码——对齐 C# System.Reflection.PropertyAttributes。
//
// **设计偏差（vs RFC 018 §4.3）**：
// RFC 原设计 `public enum PropertyAttributes { None = 0x0000, ... }` 需要枚举
// 显式值语法，但当前 Arc enum AST 不支持显式值。改用 `class + public const int`
// 常量实现，保持位掩码组合语义。（与 std/Arc/Attribute.as 中 AttributeTargets 同模式。）

namespace Arc.Reflection;

/// <summary>
/// 属性特性位掩码——对齐 C# System.Reflection.PropertyAttributes。
///
/// 设计偏差：RFC 018 §4.3 原设计为 enum + 显式值，当前 Arc enum AST 不支持
/// 显式值，故以 `class + public const int` 实现，保持位掩码组合语义。
/// 用法示例：`PropertyAttributes.None` 或 `PropertyAttributes.SpecialName`。
/// </summary>
public class PropertyAttributes {
    /// <summary>无特性（默认）。</summary>
    public const int None        = 0x0000;
    /// <summary>特殊名称属性（编译器生成的 getter/setter 配对等）。</summary>
    public const int SpecialName = 0x0200;
}
