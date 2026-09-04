// RFC 012 M3: 内置属性 — 实体表映射 [Table]（D2）。
//
// 标记类/struct 映射到数据库表，派生自 Attribute 基类。
// 与 [Column] / [Key] / [Required] / [MaxLength] 一同构成 M3 ORM 内置属性。

namespace Arc.ComponentModel;

/// <summary>
/// 标记类/struct 映射到数据库表（RFC 012 D2）。
///
/// 用法：`[Table("users")]` 或 `[Table]`（无参时由 ORM 调用方回退到类型名）。
/// 合法附加目标：class / struct。
///
/// **设计偏差**：RFC D2 原设计 `[AttributeUsage(AttributeTargets.Class | AttributeTargets.Struct)]`
/// 需要 `|` 运算符，当前 Arc parser 暂不支持属性参数中的 `|` 常量折叠。
/// M3 阶段使用 `AttributeTargets.All`（实际仅 class/struct 在 ORM 场景使用）。
/// 待 enum + `|` 运算符正式落地后回到 RFC 原设计。
/// </summary>
[AttributeUsage(AttributeTargets.All)]
public class TableAttribute : Attribute {
    /// 表名。无参 [Table] 时为 null，由 ORM 调用方回退到类型名。
    public string Name { get; }

    public TableAttribute(string name) {
        Name = name;
    }
}
