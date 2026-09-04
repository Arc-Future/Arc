// L3 骨架：EntityMapEntry — 实体映射条目。
namespace Arc.Orm;

using Arc.Collections;

/// <summary>
/// 实体映射条目——实体类型→表名+列映射。
///
/// 与泛型 EntityMap<T> 解耦：EntityMap<T> 是用户声明 API，
/// EntityMapEntry 是运行时只读快照（去泛型化，便于字典索引）。
/// </summary>
public class EntityMapEntry {
    /// <summary>实体类型名（typeof(T).FullName）。</summary>
    public string EntityTypeName { get; }

    /// <summary>数据库表名。</summary>
    public string TableName { get; }

    /// <summary>列映射集合（含主键、约束标志）。</summary>
    public List<ColumnMap> Columns { get; }

    public EntityMapEntry(string entityTypeName, string tableName) {
        this.EntityTypeName = entityTypeName;
        this.TableName = tableName;
        this.Columns = new List<ColumnMap>();
    }

    /// <summary>查找主键列；不存在返回 null。</summary>
    public ColumnMap FindKey() {
        int n = this.Columns.Count;
        int i = 0;
        while (i < n) {
            ColumnMap c = this.Columns[i];
            if (c.Flags == ColumnFlags.Key) {
                return c;
            }
            i = i + 1;
        }
        return null;
    }
}
