// RFC 018 §4.3: 方法特性位掩码——对齐 C# System.Reflection.MethodAttributes。
//
// **设计偏差（vs RFC 018 §4.3）**：
// RFC 原设计 `public enum MethodAttributes { Public = 0x0001, ... }` 需要枚举
// 显式值语法，但当前 Arc enum AST 不支持显式值。改用 `class + public const int`
// 常量实现，保持位掩码组合语义。（与 std/Arc/Attribute.as 中 AttributeTargets 同模式。）

namespace Arc.Reflection;

/// <summary>
/// 方法特性位掩码——对齐 C# System.Reflection.MethodAttributes。
///
/// 设计偏差：RFC 018 §4.3 原设计为 enum + 显式值，当前 Arc enum AST 不支持
/// 显式值，故以 `class + public const int` 实现，保持位掩码组合语义。
/// 用法示例：`MethodAttributes.Public | MethodAttributes.Static`。
/// </summary>
public class MethodAttributes {
    /// <summary>公开方法。</summary>
    public const int Public   = 0x0001;
    /// <summary>私有方法。</summary>
    public const int Private  = 0x0002;
    /// <summary>静态方法。</summary>
    public const int Static   = 0x0010;
    /// <summary>抽象方法。</summary>
    public const int Abstract = 0x0400;
    /// <summary>虚方法（virtual 或 override）。</summary>
    public const int Virtual  = 0x0040;
    /// <summary>final 方法（不可被子类覆写）。</summary>
    public const int Final    = 0x0020;
}
