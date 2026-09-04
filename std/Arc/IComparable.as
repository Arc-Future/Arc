namespace Arc;

/// 比较接口——定义类型实例之间的全序关系（对标 C# System.IComparable<T>）。
///
/// 泛型约束 `where T : IComparable<T>` 要求类型实现本接口；
/// 基元类型（int/double/string 等）由编译器内置视为已实现，无需显式声明。
///
/// RFC 004 M1 扩展：新增 `static abstract int Compare(T a, T b)` 成员——
/// 为排序算法提供零装箱比较基础。基元类型由编译器内置实现，
/// codegen 在 `try_emit_primitive_static` 拦截器中直接发射 LLVM `icmp`/
/// `fcmp` + 三值化指令。实例 `CompareTo` 保留用于既有调用约定。
public interface IComparable<T> {
    /// <summary>将当前实例与指定对象比较，返回相对顺序。</summary>
    /// <param name="other">待比较的对象。</param>
    /// <returns>小于零表示当前实例较小；零表示相等；大于零表示当前实例较大。</returns>
    int CompareTo(T other);

    /// <summary>静态比较两个值，返回相对顺序（RFC 004 M1）。</summary>
    /// <param name="a">第一个值。</param>
    /// <param name="b">第二个值。</param>
    /// <returns>负数表示 a 较小；零表示相等；正数表示 a 较大。</returns>
    static abstract int Compare(T a, T b);
}
