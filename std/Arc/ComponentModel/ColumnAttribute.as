// RFC 012 M3: 内置属性 — 实体列映射 [Column]（D2）。
//
// 标记 property/field 映射到数据库列，派生自 Attribute 基类。

namespace Arc.ComponentModel;

/// <summary>
/// 标记 property/field 映射到数据库列（RFC 012 D2）。
///
/// 用法：`[Column("age")]` 或 `[Column]`（无参时由 ORM 调用方回退到字段名）。
/// 合法附加目标：property / field。
/// </summary>
[AttributeUsage(AttributeTargets.All)]
public class ColumnAttribute : Attribute {
    /// 列名。无参 [Column] 时为 null，由 ORM 调用方回退到字段名。
    public string Name { get; }

    public ColumnAttribute(string name) {
        Name = name;
    }
}
