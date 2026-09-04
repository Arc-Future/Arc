// RFC 012 M3: Attribute 约束体系根基类型。
//
// 本文件定义所有特性的根基类与元属性：
//   - Attribute：所有特性（内置 / 用户自定义 / 宏特性）的根基类
//   - AttributeTargets：可附加目标的位掩码常量集合
//   - AttributeUsageAttribute：标记 attribute 类的附加规则元属性
//
// **设计偏差（vs RFC D7.3）**：
// RFC 原设计 `public enum AttributeTargets { Class = 0x0001, ... }` 需要枚举显式值
// 语法，但当前 Arc enum AST 不支持显式值（仅支持命名变体）。为保持位掩码组合
// 语义（`AttributeTargets.Class | AttributeTargets.Struct`），改用 `class` +
// `public const int` 常量实现。语义与 RFC 等价：typeck 在编译期识别这些常量
// 名并按位掩码消费。未来 enum 显式值语法落地后可回到 RFC 原设计。
//
// **架构红线**（RFC 012 D4.1/D6.1）：
//   - Attribute 仅为类型系统锚点，运行时不反射
//   - typeck 识别 `class FooAttribute : Attribute` 派生类为「属性类型」
//   - 不得作为非属性类使用（typeck 校验）

namespace Arc;

/// <summary>
/// 所有特性的根基类（RFC 012 D7.1）。
///
/// 内置属性、用户自定义属性、宏特性（GenerateToAttribute /
/// GenerateToAttribute&lt;T&gt; / SourceGeneratorAttribute）均派生自此。
/// 无成员，纯标记基类。typeck 识别 `class FooAttribute : Attribute`
/// 派生类为「属性类型」；不得作为非属性类使用。
/// </summary>
public class Attribute {
    public Attribute() {}
}

/// <summary>
/// 特性可附加的声明目标位掩码常量（RFC 012 D7.3）。
///
/// 设计偏差：RFC 原设计为 enum + 显式值，当前 Arc enum AST 不支持显式值，
/// 故以 `class + public const int` 实现，保持位掩码组合语义。
/// 用法示例：`AttributeTargets.Class | AttributeTargets.Struct`。
/// </summary>
public class AttributeTargets {
    public const int Class       = 1;
    public const int Struct      = 2;
    public const int Interface   = 4;
    public const int Enum        = 8;
    public const int Method      = 16;
    public const int Property    = 32;
    public const int Field       = 64;
    public const int Parameter   = 128;
    public const int EnumMember  = 256;
    public const int Assembly    = 512;
    public const int All         = 511;
    /// Method | Property——供同时附着于方法与属性访问器的特性（如 [Builtin]）。
    public const int MethodOrProperty = 48;
}

/// <summary>
/// 标记 attribute 类的可附加目标与重复性规则（RFC 012 D7.2）。
///
/// 附加到 attribute 类（如 `[AttributeUsage(AttributeTargets.Class)]`），
/// 声明该属性类的合法附加目标、是否允许重复附加、派生类是否继承基类属性。
/// 未标注 `[AttributeUsage]` 的 attribute 类默认 ValidOn=All、
/// AllowMultiple=false、Inherited=true。
/// </summary>
[AttributeUsage(AttributeTargets.Class)]
public class AttributeUsageAttribute : Attribute {
    /// 合法附加目标（位掩码，AttributeTargets.* 组合）。
    public int ValidOn { get; }
    /// 是否允许同一符号重复附加（默认 false）。
    public bool AllowMultiple { get; set; }
    /// 派生类是否继承基类的属性（默认 true）。
    public bool Inherited { get; set; }

    public AttributeUsageAttribute(int validOn) {
        ValidOn = validOn;
        Inherited = true;
    }
}
