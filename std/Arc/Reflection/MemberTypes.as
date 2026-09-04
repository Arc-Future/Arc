// RFC 018 §4.3: 成员类型枚举——对齐 C# System.Reflection.MemberTypes。
//
// **设计偏差（vs RFC 018 §4.3）**：
// RFC 原设计 `public enum MemberTypes { TypeInfo = 1, ... }` 需要枚举显式值
// 语法，但当前 Arc enum AST 不支持显式值（仅支持命名变体）。为保持位掩码
// 组合语义，改用 `class + public const int` 常量实现。语义与 RFC 等价：
// typeck 在编译期识别这些常量名并按位掩码消费。未来 enum 显式值语法落地后
// 可回到 RFC 原设计。（与 std/Arc/Attribute.as 中 AttributeTargets 同模式。）

namespace Arc.Reflection;

/// <summary>
/// 成员类型枚举——对齐 C# System.Reflection.MemberTypes。
///
/// 设计偏差：RFC 018 §4.3 原设计为 enum + 显式值，当前 Arc enum AST 不支持
/// 显式值，故以 `class + public const int` 实现，保持位掩码组合语义。
/// 用法示例：`MemberTypes.Method | MemberTypes.Constructor`。
/// </summary>
public class MemberTypes {
    /// <summary>类型信息（Type/TypeInfo 成员）。</summary>
    public const int TypeInfo    = 1;
    /// <summary>方法成员（MethodInfo）。</summary>
    public const int Method      = 2;
    /// <summary>字段成员（FieldInfo）。</summary>
    public const int Field       = 4;
    /// <summary>属性成员（PropertyInfo）。</summary>
    public const int Property    = 8;
    /// <summary>事件成员（EventInfo）。</summary>
    public const int Event       = 16;
    /// <summary>构造函数成员（ConstructorInfo）。</summary>
    public const int Constructor = 32;
    /// <summary>嵌套类型成员。</summary>
    public const int NestedType  = 64;
}
