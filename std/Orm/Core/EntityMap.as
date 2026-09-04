// RFC 039 M2 MVP: 实体→表/列映射容器。
//
// 提供 EntityMap<T> 数据结构，供用户手动声明实体到数据库表的映射契约。
// MVP 阶段不依赖 typeck 属性消费——属性（[Table]/[Column]/[Key]/[Required]/
// [MaxLength]）当前仅作声明性文档，codegen 不为属性生成任何 IR
// （RFC 039 D4.2）；M4 GenerateToAttribute 宏特性落地后，本文件的
// EntityMap 声明将由编译期自动生成。
//
// 架构红线（RFC 039 D4.1/D6.1）：
//   - T 仅为编译期类型标记，运行时不反射
//   - 不引入 rt_orm_* runtime ABI
//   - 不修改编译器核心 7 crate
namespace Arc.Orm;

using Arc.Collections;

/// 实体→表/列映射（RFC 039 D4.1）。
///
/// 泛型参数 T 仅为编译期类型标记，用于在用户代码中关联实体类型；
/// 运行时不反射 T 的成员——所有列映射由用户通过流式 API 手动声明。
///
/// 使用示例：
///   var userMap = new EntityMap<User>("users")
///       .KeyColumn("Id", "id")
///       .RequiredColumn("Name", "name")
///       .Column("Age", "age");
public class EntityMap<T> {
    public string TableName;
    public List<ColumnMap> Columns;

    public EntityMap(string tableName) {
        this.TableName = tableName;
        this.Columns = new List<ColumnMap>();
    }

    /// 添加普通列映射（无约束标志）。
    public EntityMap<T> Column(string propertyName, string columnName) {
        Columns.Add(new ColumnMap(propertyName, columnName));
        return this;
    }

    /// 添加带 [Key] 标志的列（对标 RFC 039 §D2 KeyAttribute）。
    public EntityMap<T> KeyColumn(string propertyName, string columnName) {
        ColumnMap c = new ColumnMap(propertyName, columnName);
        c.Flags = ColumnFlags.Key;
        Columns.Add(c);
        return this;
    }

    /// 添加带 [Required] 标志的列（对标 RFC 039 §D2 RequiredAttribute）。
    public EntityMap<T> RequiredColumn(string propertyName, string columnName) {
        ColumnMap c = new ColumnMap(propertyName, columnName);
        c.Flags = ColumnFlags.Required;
        Columns.Add(c);
        return this;
    }

    /// 添加带 MaxLength 约束的列（对标 RFC 039 §D2 MaxLengthAttribute）。
    public EntityMap<T> MaxLengthColumn(string propertyName, string columnName, int maxLength) {
        ColumnMap c = new ColumnMap(propertyName, columnName);
        c.MaxLength = maxLength;
        Columns.Add(c);
        return this;
    }

    /// 查找主键列；不存在返回 null。
    public ColumnMap FindKey() {
        int n = Columns.Count;
        int i = 0;
        while (i < n) {
            var c = Columns[i];
            if (c.Flags == ColumnFlags.Key) {
                return c;
            }
            i = i + 1;
        }
        return null;
    }
}
