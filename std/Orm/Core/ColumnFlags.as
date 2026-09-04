namespace Arc.Orm;

/// 列约束标志（RFC 039 D4.1，对应 [Key]/[Required] 内置属性）。
public enum ColumnFlags {
    None,
    Key,
    Required,
}
