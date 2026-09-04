// RFC 039 M2 MVP: 列映射条目（实体属性 → 数据库列的映射描述）。
namespace Arc.Orm;

/// <summary>
/// 列映射条目（RFC 039 D4.1）。
///
/// 描述实体属性到数据库列的映射：属性名、列名、约束标志、MaxLength。
/// 注意：当前为 class（Phase B 编译通过），待 typeck 修复 struct 构造函数
/// 类型推断问题后改回 struct，恢复栈分配零开销设计。
/// </summary>
public class ColumnMap {
    public string PropertyName;
    public string ColumnName;
    public ColumnFlags Flags;
    /// <summary>MaxLength 约束值；0 表示未设置（对齐 [MaxLength] 属性）。</summary>
    public int MaxLength;

    public ColumnMap(string propertyName, string columnName) {
        this.PropertyName = propertyName;
        this.ColumnName = columnName;
        this.Flags = ColumnFlags.None;
        this.MaxLength = 0;
    }
}
