// RFC 018 §4.3: 类型分类枚举——Arc 简化（C# 无对应枚举，用 Is* 属性表达）。
//
// 与其它 *Attributes / MemberTypes 不同，TypeKind 无显式值需求（按声明顺序），
// 可直接使用 Arc 标准 enum 语法。

namespace Arc.Reflection;

/// <summary>
/// 类型分类枚举——Arc 简化（C# 无对应枚举，用 Is* 属性表达）。
///
/// 表示类型的宏观分类，供 Type.Kind 字段使用。无显式值需求（按声明顺序），
/// 故直接使用 Arc 标准 enum 语法，无需 class + const int 模式。
/// </summary>
public enum TypeKind {
    /// <summary>基元类型（int/long/short/byte/char/float/double/bool/string/void）。</summary>
    Primitive,
    /// <summary>class（引用类型）。</summary>
    Class,
    /// <summary>struct（值类型，非 enum）。</summary>
    Struct,
    /// <summary>interface。</summary>
    Interface,
    /// <summary>enum。</summary>
    Enum,
    /// <summary>数组类型。</summary>
    Array,
    /// <summary>可空类型（T?）。</summary>
    Nullable,
    /// <summary>Task 类型。</summary>
    Task,
    /// <summary>Func/Action 委托类型。</summary>
    Func,
    /// <summary>其它类型（不属于以上分类）。</summary>
    Other,
}
