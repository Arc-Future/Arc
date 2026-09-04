// RFC 039 Phase B: 实体状态枚举（对标 EFCore EntityState）。
//
// ChangeTracker 通过此枚举标识被追踪实体的状态，SaveChangesAsync 时
// 按状态生成对应的 INSERT/UPDATE/DELETE 语句。
namespace Arc.Orm;

/// 实体状态枚举——标识被追踪实体相对数据库的状态。
public enum EntityState {
    /// <summary>未追踪——ChangeTracker 不管理此实体。</summary>
    Detached,
    /// <summary>未变更——与数据库一致，SaveChangesAsync 跳过。</summary>
    Unchanged,
    /// <summary>已添加——SaveChangesAsync 生成 INSERT。</summary>
    Added,
    /// <summary>已修改——SaveChangesAsync 生成 UPDATE。</summary>
    Modified,
    /// <summary>已删除——SaveChangesAsync 生成 DELETE。</summary>
    Deleted,
}
