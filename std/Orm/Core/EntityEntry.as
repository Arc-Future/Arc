// L3 骨架：EntityEntry — 变更追踪条目。
namespace Arc.Orm;

/// <summary>
/// 实体追踪条目——ChangeTracker 内部存储单元（class 引用，便于 List 存储）。
/// </summary>
public class EntityEntry {
    /// <summary>被追踪实体（弱引用语义，由调用方保证非 null）。</summary>
    public object Entity;

    /// <summary>实体状态。</summary>
    public EntityState State;

    /// <summary>实体类型名（供 SaveChangesAsync 查找 EntityMapEntry 用）。</summary>
    public string EntityTypeName;

    public EntityEntry(object entity, EntityState state, string entityTypeName) {
        this.Entity = entity;
        this.State = state;
        this.EntityTypeName = entityTypeName;
    }
}
