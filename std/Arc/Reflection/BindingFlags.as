// RFC 018 §4.3: 绑定标志——对齐 C# System.Reflection.BindingFlags。
//
// 用于 GetMethods/GetFields 等方法的过滤参数（M1+ 阶段 GetXxx(BindingFlags) 重载使用）。
//
// **设计偏差（vs RFC 018 §4.3）**：
// RFC 原设计 `public enum BindingFlags { Default = 0, ... }` 需要枚举显式值
// 语法，但当前 Arc enum AST 不支持显式值。改用 `class + public const int`
// 常量实现，保持位掩码组合语义。（与 std/Arc/Attribute.as 中 AttributeTargets 同模式。）

namespace Arc.Reflection;

/// <summary>
/// 绑定标志——对齐 C# System.Reflection.BindingFlags。
///
/// 用于 GetMethods/GetFields 等方法的过滤参数（M1+ 阶段 GetXxx(BindingFlags) 重载使用）。
///
/// 设计偏差：RFC 018 §4.3 原设计为 enum + 显式值，当前 Arc enum AST 不支持
/// 显式值，故以 `class + public const int` 实现，保持位掩码组合语义。
/// 用法示例：`BindingFlags.Public | BindingFlags.Instance`。
/// </summary>
public class BindingFlags {
    /// <summary>默认绑定（不指定过滤条件）。</summary>
    public const int Default      = 0;
    /// <summary>包含 public 成员。</summary>
    public const int Public       = 0x0100;
    /// <summary>包含非 public 成员（private/protected 等）。</summary>
    public const int NonPublic    = 0x0200;
    /// <summary>包含静态成员。</summary>
    public const int Static       = 0x0008;
    /// <summary>包含实例成员。</summary>
    public const int Instance     = 0x0004;
    /// <summary>仅本类型声明的成员（不含继承）。</summary>
    public const int DeclaredOnly = 0x0002;
}
