// L3 骨架：DbSet<T> — 实体集门面（对标 EF Core DbSet 表面）。
//
// 继承 EntityQueryable<T> 获得同步链式查询骨架；Add/Update/Remove 委托 ChangeTracker。
// ToList / 异步执行 / SaveChanges 写库均未落地——禁止冒充完整 CRUD。
namespace Arc.Orm;

using Arc.Collections;
using Arc.Data;
using Arc.Linq.Expressions;

public class DbSet<T> : EntityQueryable<T> {
    /// <summary>变更追踪器（来自 DbContext，scoped 私有）。</summary>
    private ChangeTracker _changeTracker;

    /// <summary>实体类型名（ChangeTracker 索引用）。</summary>
    private string _entityTypeName;

    /// <summary>
    /// 构造 DbSet。
    /// </summary>
    /// <param name="changeTracker">DbContext 的变更追踪器。</param>
    /// <param name="provider">数据库提供者。</param>
    /// <param name="expression">表名根表达式。</param>
    /// <param name="entityTypeName">实体类型名（用于 ChangeTracker 索引）。</param>
    public DbSet(ChangeTracker changeTracker, IDbProvider provider, Expression expression, string entityTypeName)
        : base(provider, expression) {
        _changeTracker = changeTracker;
        _entityTypeName = entityTypeName;
    }

    // ── 变更追踪（委托给 ChangeTracker，零装箱 struct 操作）──

    /// <summary>标记实体为 Added，SaveChangesAsync 时生成 INSERT。</summary>
    public void Add(T entity) {
        _changeTracker.Add(entity, _entityTypeName);
    }

    /// <summary>标记实体为 Modified，SaveChangesAsync 时生成 UPDATE。</summary>
    public void Update(T entity) {
        _changeTracker.Update(entity, _entityTypeName);
    }

    /// <summary>标记实体为 Deleted，SaveChangesAsync 时生成 DELETE。</summary>
    public void Remove(T entity) {
        _changeTracker.Remove(entity, _entityTypeName);
    }
}
