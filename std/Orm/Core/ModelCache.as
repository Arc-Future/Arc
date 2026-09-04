// L3 骨架：ModelCache — DbContext 类型名 → FrozenModel 静态缓存。
//
// 可证伪：Get/Set/Clear 内存路径。禁止与 EFCore 性能对照宣称。
namespace Arc.Orm;

using Arc.Collections.Concurrent;

/// <summary>
/// 模型缓存——元数据共享的核心机制。
///
/// 静态类，所有 DbContext 实例共享同一 FrozenModel 快照。
/// 首次构建后冻结，后续读取无锁、零反射、零重建。
/// </summary>
public class ModelCache {
    /// <summary>缓存表——DbContext 类型名 → FrozenModel。
    /// 高并发安全：ConcurrentDictionary per-bucket lock，读写均线程安全。</summary>
    private static ConcurrentDictionary<string, FrozenModel> _cache = new ConcurrentDictionary<string, FrozenModel>();

    /// <summary>按 DbContext 类型名获取已缓存的模型（不触发构建）。</summary>
    /// <param name="contextTypeName">DbContext 子类类型名。</param>
    /// <returns>冻结的模型快照；未构建返回 null。</returns>
    public static FrozenModel Get(string contextTypeName) {
        return _cache.GetValueOrDefault(contextTypeName);
    }

    /// <summary>写入模型缓存（首次构建后调用）。</summary>
    /// <param name="contextTypeName">DbContext 子类类型名。</param>
    /// <param name="model">冻结的模型快照。</param>
    public static void Set(string contextTypeName, FrozenModel model) {
        _cache[contextTypeName] = model;
    }

    /// <summary>清空缓存（仅供测试用例隔离，生产不应调用）。</summary>
    public static void Clear() {
        _cache.Clear();
    }
}
