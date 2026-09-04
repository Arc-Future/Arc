// L3 骨架：CompiledQueryCache — 按哈希缓存 SQL 字符串。
//
// 可证伪：GetOrTranslate 命中/未命中路径（传入 expressionHash + translator）。
// 非 stub 空壳，但 ≠ 产品级查询编译；禁止与 EFCore 对照宣称。
namespace Arc.Orm;

using Arc.Collections.Concurrent;

/// <summary>
/// 编译查询缓存——避免重复 SQL 翻译。
///
/// 缓存键：Expression 节点根引用的哈希值（避免深比较开销）。
/// 缓存值：翻译后的 SQL 字符串 + 参数绑定信息。
/// </summary>
public class CompiledQueryCache {
    /// <summary>缓存表——表达式哈希 → SQL 字符串。
    /// 高并发安全：ConcurrentDictionary 保证多线程读写无数据结构损坏。</summary>
    private static ConcurrentDictionary<int, string> _sqlCache = new ConcurrentDictionary<int, string>();

    /// <summary>
    /// 获取或翻译 SQL——首次调用翻译并缓存，后续直接返回。
    ///
    /// 缓存键为表达式树的哈希值（由 codegen 注入）。
    /// </summary>
    /// <param name="expressionHash">表达式树哈希（由 codegen 注入）。</param>
    /// <param name="translator">翻译工厂（仅缓存未命中时执行）。</param>
    /// <returns>翻译后的 SQL 字符串。</returns>
    public static string GetOrTranslate(int expressionHash, Func<string> translator) {
        string cached = _sqlCache.GetValueOrDefault(expressionHash);
        if (cached != null) {
            return cached;
        }
        string sql = translator();
        _sqlCache[expressionHash] = sql;
        return sql;
    }

    /// <summary>清空缓存（仅供测试用例隔离）。</summary>
    public static void Clear() {
        _sqlCache.Clear();
    }
}
