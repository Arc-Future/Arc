namespace Arc;

/// 值相等性接口（RFC 004 M1）。
///
/// 替代 RFC 016 v1 `Object.Equals(object)` 的装箱语义——
/// 泛型方法 `where T : IEquatable<T>` 约束确保编译期类型已知，
/// 单态化后直接调用基元 `Equals` 指令或用户实现的 `Type_Equals` 静态方法，
/// 零装箱、零虚分派。
///
/// 基元类型（int/long/short/byte/float/double/bool/char/string）由
/// 编译器内置隐式实现，codegen 在 `try_emit_primitive_static` 拦截器中
/// 直接发射 LLVM `icmp`/`fcmp` 指令。
public interface IEquatable<T> {
    /// <summary>判断两个值是否相等。</summary>
    /// <param name="a">第一个值。</param>
    /// <param name="b">第二个值。</param>
    /// <returns>相等返回 true；否则返回 false。</returns>
    static abstract bool Equals(T a, T b);
}
