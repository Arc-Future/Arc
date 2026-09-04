// L3 骨架：MaterializerCache — 物化器缓存。
namespace Arc.Orm;

using Arc.Collections.Concurrent;

/// <summary>
/// 物化器缓存——按实体类型名索引。
///
/// codegen 在 DbContext 子类构造时注入专用物化器。
/// 运行时通过实体类型名查表获取物化器（O(1) 字典查找）。
///
/// 高并发安全：ConcurrentDictionary per-bucket lock，多 DbContext 实例并发查表安全。
/// </summary>
public class MaterializerCache {
    /// <summary>缓存表——实体类型名 → 物化器。
    /// 高并发安全：ConcurrentDictionary 保证多线程读写无数据结构损坏。</summary>
    private static ConcurrentDictionary<string, IEntityMaterializer> _cache = new ConcurrentDictionary<string, IEntityMaterializer>();

    /// <summary>注册专用物化器（由 codegen 在 DbContext 构造时调用）。</summary>
    /// <param name="entityTypeName">实体类型名。</param>
    /// <param name="materializer">物化器实例。</param>
    public static void Register(string entityTypeName, IEntityMaterializer materializer) {
        _cache[entityTypeName] = materializer;
    }

    /// <summary>按实体类型名查找物化器。</summary>
    /// <param name="entityTypeName">实体类型名。</param>
    /// <returns>物化器实例；未注册返回 null。</returns>
    public static IEntityMaterializer Get(string entityTypeName) {
        return _cache.GetValueOrDefault(entityTypeName);
    }
}
