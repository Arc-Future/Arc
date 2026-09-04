// L3 骨架：FrozenModel — 只读模型快照（跨 DbContext 实例共享目标架构）。
//
// 架构红线：无运行时反射；无 rt_orm_* ABI；纯 Arc 数据结构。
namespace Arc.Orm;

using Arc.Collections;

/// <summary>
/// 冻结模型——只读快照，跨 DbContext 实例共享。
///
/// 由 ModelCache.GetOrBuild 首次构建后冻结。所有 DbContext 实例通过
/// ModelCache.Get(ctxType) 读取同一快照，零重建开销、零锁竞争。
/// </summary>
public class FrozenModel {
    /// <summary>DbContext 子类类型名（缓存键）。</summary>
    public string ContextTypeName { get; }

    /// <summary>实体→表/列映射集合（按实体类型名索引）。</summary>
    public List<EntityMapEntry> EntityMaps { get; }

    /// <summary>实体类型名→EntityMaps 索引（O(1) 查表，零迭代）。</summary>
    private Dictionary<string, int> _entityIndex;

    public FrozenModel(string contextTypeName) {
        this.ContextTypeName = contextTypeName;
        this.EntityMaps = new List<EntityMapEntry>();
        _entityIndex = new Dictionary<string, int>();
    }

    /// <summary>添加实体映射（仅在构建阶段调用，构建后不再修改）。</summary>
    /// <param name="entityTypeName">实体类型名（typeof(T).FullName）。</param>
    /// <param name="tableName">数据库表名。</param>
    public void AddEntityMap(string entityTypeName, string tableName) {
        int idx = this.EntityMaps.Count;
        this.EntityMaps.Add(new EntityMapEntry(entityTypeName, tableName));
        _entityIndex[entityTypeName] = idx;
    }

    /// <summary>按实体类型名查找映射（O(1) 查表，零迭代）。</summary>
    /// <param name="entityTypeName">实体类型名。</param>
    /// <returns>映射条目；不存在返回 null。</returns>
    public EntityMapEntry FindMap(string entityTypeName) {
        if (_entityIndex.ContainsKey(entityTypeName)) {
            int idx = _entityIndex[entityTypeName];
            return this.EntityMaps[idx];
        }
        return null;
    }
}
