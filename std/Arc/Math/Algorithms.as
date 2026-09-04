namespace Arc;

/// <summary>
/// 泛型算法库（RFC 004 M3）。
///
/// 基于 `INumber<T>`/`IComparable<T>` static abstract 接口约束，
/// 单态化后直接发射具体类型的运算指令或 `Type_Method` 静态调用，
/// 零装箱、零虚分派。基元类型（int/double 等）编译器隐式实现接口，
/// 用户自定义类型显式实现接口即可复用全部算法。
///
/// 不在本文件范围的算法（deferred 到后续迭代）：
/// - `Average<T>`：需要 `int → T` 转换接口（未来 `ICreatableFromInt<T>`）
/// - `Sort<T>`：算法本身 deferred（`T[]` 元素赋值已由 MIR `IndexSet` 支持）
/// </summary>

/// <summary>求数组所有元素的和。</summary>
/// <param name="items">待求和的数组。</param>
/// <returns>数组元素的和；空数组返回 <c>T.Zero</c>。</returns>
public T Sum<T>(T[] items) where T : INumber<T> {
    T sum = T.Zero;
    int n = items.Length;
    int i = 0;
    while (i < n) {
        sum = T.Add(sum, items[i]);
        i = i + 1;
    }
    return sum;
}

/// <summary>求数组中的最小值。</summary>
/// <param name="items">待求最小值的数组。</param>
/// <returns>数组元素的最小值；空数组返回 <c>T.Zero</c>。</returns>
public T Min<T>(T[] items) where T : INumber<T>, IComparable<T> {
    int n = items.Length;
    if (n == 0) {
        return T.Zero;
    }
    T result = items[0];
    int i = 1;
    while (i < n) {
        if (T.Compare(items[i], result) < 0) {
            result = items[i];
        }
        i = i + 1;
    }
    return result;
}

/// <summary>求数组中的最大值。</summary>
/// <param name="items">待求最大值的数组。</param>
/// <returns>数组元素的最大值；空数组返回 <c>T.Zero</c>。</returns>
public T Max<T>(T[] items) where T : INumber<T>, IComparable<T> {
    int n = items.Length;
    if (n == 0) {
        return T.Zero;
    }
    T result = items[0];
    int i = 1;
    while (i < n) {
        if (T.Compare(items[i], result) > 0) {
            result = items[i];
        }
        i = i + 1;
    }
    return result;
}
