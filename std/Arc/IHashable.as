namespace Arc;

/// 哈希接口（RFC 004 M1）。
///
/// 替代 RFC 016 v1 `Object.GetHashCode()` 的装箱语义——
/// 为 `Dictionary<K,V>` 提供零装箱哈希基础。
/// 泛型方法 `where T : IHashable<T>` 约束确保编译期类型已知，
/// 单态化后直接调用基元哈希指令或用户实现的 `Type_GetHashCode` 静态方法。
///
/// 基元类型由编译器内置隐式实现：int/long 直接返回值（截断到 32 位），
/// float/double 转为 bit pattern 后哈希，string 调用 rt_string_hash。
///
/// **自定义键类型范本**（与 `IEquatable&lt;T&gt;` 成对实现，供 Dictionary/ConcurrentDictionary
/// 作键；用 `HashCode` 辅助安全组合多字段哈希）：
/// <code>
/// public class Point : IEquatable&lt;Point&gt;, IHashable&lt;Point&gt; {
///     public int X;
///     public int Y;
///
///     public static bool Equals(Point a, Point b) {
///         return a.X == b.X && a.Y == b.Y;
///     }
///
///     public static int GetHashCode(Point value) {
///         return HashCode.Combine(
///             HashCode.HashValue(value.X),
///             HashCode.HashValue(value.Y));
///     }
/// }
/// </code>
public interface IHashable<T> {
    /// <summary>计算值的哈希码。</summary>
    /// <param name="value">待哈希的值。</param>
    /// <returns>32 位哈希码。</returns>
    static abstract int GetHashCode(T value);
}
