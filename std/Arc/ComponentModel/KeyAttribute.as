// RFC 012 M3: 内置属性 — 主键标记 [Key]（D2）。
//
// 标记 property/field 为主键，派生自 Attribute 基类。

namespace Arc.ComponentModel;

/// <summary>
/// 标记 property/field 为主键（RFC 012 D2）。
///
/// 用法：`[Key]`（无参数）。
/// 合法附加目标：property / field。
/// </summary>
[AttributeUsage(AttributeTargets.All)]
public class KeyAttribute : Attribute {
    public KeyAttribute() {}
}
