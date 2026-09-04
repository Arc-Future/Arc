// RFC 018 §4.3: 参数特性位掩码——对齐 C# System.Reflection.ParameterAttributes。
//
// **设计偏差（vs RFC 018 §4.3）**：
// RFC 原设计 `public enum ParameterAttributes { None = 0x0000, ... }` 需要枚举
// 显式值语法，但当前 Arc enum AST 不支持显式值。改用 `class + public const int`
// 常量实现，保持位掩码组合语义。（与 std/Arc/Attribute.as 中 AttributeTargets 同模式。）

namespace Arc.Reflection;

/// <summary>
/// 参数特性位掩码——对齐 C# System.Reflection.ParameterAttributes。
///
/// 设计偏差：RFC 018 §4.3 原设计为 enum + 显式值，当前 Arc enum AST 不支持
/// 显式值，故以 `class + public const int` 实现，保持位掩码组合语义。
/// 用法示例：`ParameterAttributes.In | ParameterAttributes.Out`。
/// </summary>
public class ParameterAttributes {
    /// <summary>无特性（默认）。</summary>
    public const int None     = 0x0000;
    /// <summary>in 参数（readonly ref）。</summary>
    public const int In       = 0x0001;
    /// <summary>out 参数。</summary>
    public const int Out      = 0x0002;
    /// <summary>可选参数（有默认值）。</summary>
    public const int Optional = 0x0010;
}
