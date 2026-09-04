// RFC 024 M1: Arc.Collections.Concurrent — 线程安全字典 facade。
// 对标 C# System.Collections.Concurrent.ConcurrentDictionary<TKey,TValue>。
//
// 内部实现：per-bucket mutex + lock-free read + epoch-based safe reclamation。
// 编译期单态化零虚分派（K.Hash/Equals 编译期确定目标函数）。
//
// [Builtin(ABI = "...")] 方法为 codegen stub——方法体不执行，
// codegen 拦截后直接发射 @rt_concurrent_dict_* ABI 调用。
namespace Arc.Collections.Concurrent;

using Arc;

/// <summary>
/// 线程安全字典。约束 K : IEquatable&lt;K&gt;, IHashable&lt;K&gt; 零装箱强制。
/// </summary>
public class ConcurrentDictionary<K, V>
    where K : IEquatable<K>, IHashable<K>
{
    private int _handle;

    // ── 构造函数 ──

    /// <summary>构造空字典（默认 31 桶）。</summary>
    [Builtin(ABI = "rt_concurrent_dict_create")]
    public ConcurrentDictionary() { _handle = 0; }

    /// <summary>构造空字典，指定并发级别（桶数自动取最近素数）。</summary>
    [Builtin(ABI = "rt_concurrent_dict_create_level")]
    public ConcurrentDictionary(int concurrencyLevel) { _handle = 0; }

    /// <summary>构造空字典，指定并发级别和初始容量。</summary>
    [Builtin(ABI = "rt_concurrent_dict_create_level_cap")]
    public ConcurrentDictionary(int concurrencyLevel, int capacity) { _handle = 0; }

    // ── 原子操作 ──

    /// <summary>尝试添加键值对。键已存在返回 false。</summary>
    [Builtin(ABI = "rt_concurrent_dict_try_add")]
    public bool TryAdd(K key, V value) { return false; }

    /// <summary>尝试获取值。存在返回 true 并输出值。</summary>
    [Builtin(ABI = "rt_concurrent_dict_try_get")]
    public bool TryGetValue(K key, out V value) { value = default(V); return false; }

    /// <summary>尝试更新——仅当现有值等于 comparisonValue 时才更新为 newValue（CAS）。</summary>
    [Builtin(ABI = "rt_concurrent_dict_try_update")]
    public bool TryUpdate(K key, V newValue, V comparisonValue) { return false; }

    // ── 索引器（C# this[K]；RFC 007，单一惯用）──
    // codegen 拦截 get_Item / set_Item → rt_concurrent_dict_{get_or_default,set}（非空 stub）

    /// <summary>索引器：读取/写入键对应的值（<c>dict[k]</c>）。键不存在时 get 返回 default(V)。</summary>
    public V this[K key] {
        get { return default(V); }
        set { }
    }

    /// <summary>获取值；键不存在返回 default(V)（C# <c>GetValueOrDefault</c>）。</summary>
    [Builtin(ABI = "rt_concurrent_dict_get_or_default")]
    public V GetValueOrDefault(K key) { return default(V); }

    /// <summary>尝试移除键。成功返回 true 并输出值。</summary>
    [Builtin(ABI = "rt_concurrent_dict_try_remove")]
    public bool TryRemove(K key, out V value) { value = default(V); return false; }

    /// <summary>获取或添加。键不存在时直接写入 value（无 delegate）。</summary>
    /// <remarks>
    /// <c>GetOrAdd(Func)</c> / <c>AddOrUpdate</c> 需 Arc Func→C trampoline——已从 Stable 面撤下（禁空 stub 挂面）；
    /// 请用本重载或 TryAdd/TryUpdate 组合。
    /// </remarks>
    [Builtin(ABI = "rt_concurrent_dict_get_or_add_val")]
    public V GetOrAdd(K key, V value) { return default(V); }

    // ── 集合属性 ──

    /// <summary>判断是否包含键。</summary>
    [Builtin(ABI = "rt_concurrent_dict_contains")]
    public bool ContainsKey(K key) { return false; }

    /// <summary>元素总数（近似值，非原子快照）。</summary>
    [Builtin(ABI = "rt_concurrent_dict_count")]
    public int Count { get; }

    /// <summary>是否为空。</summary>
    public bool IsEmpty { get { return this.Count == 0; } }

    /// <summary>清空所有元素。</summary>
    [Builtin(ABI = "rt_concurrent_dict_clear")]
    public void Clear() { }

    // ── 键快照 ──
    // Values / ToArray：runtime 以 void* 槽存放（标量为 inttoptr）；
    // int[] / KeyValuePair[] 布局未对齐 → 不挂 Arc Stable 面（C ABI e2e 仍覆盖）。

    /// <summary>所有键的数组快照（非原子；引用型键如 string 可直接索引）。</summary>
    [Builtin(ABI = "rt_concurrent_dict_keys")]
    public K[] Keys { get; }
}
