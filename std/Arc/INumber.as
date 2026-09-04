namespace Arc;

/// 数值运算通用接口（RFC 004 M1）。
///
/// 基元类型（int/long/short/byte/float/double）由编译器内置隐式实现——
/// 用户源码不可见，codegen 在 `try_emit_primitive_static` 拦截器中
/// 直接发射 LLVM 运算指令（零运行时开销）。
/// 用户自定义数值类型可显式实现本接口，走普通静态方法调用 codegen 路径。
///
/// 本 RFC 遵守项目硬约束『运算符重载禁止』——使用 `T.Add(a, b)` 方法形式，
/// 不引入 `a + b` 运算符语法糖。
public interface INumber<T> {
    /// <summary>加法：a + b。</summary>
    static abstract T Add(T a, T b);
    /// <summary>减法：a - b。</summary>
    static abstract T Subtract(T a, T b);
    /// <summary>乘法：a * b。</summary>
    static abstract T Multiply(T a, T b);
    /// <summary>除法：a / b。</summary>
    static abstract T Divide(T a, T b);
    /// <summary>取负：-a。</summary>
    static abstract T Negate(T a);
    /// <summary>零值常量。</summary>
    static abstract T Zero { get; }
    /// <summary>单位元常量。</summary>
    static abstract T One { get; }
}

/// 加法接口（细粒度，RFC 004 §12 推荐拆分）。
/// 允许类型仅实现部分运算（如 `string : IAddable<string>` 但不实现 `IMultiplicable<string>`）。
public interface IAddable<T> {
    static abstract T Add(T a, T b);
}

/// 减法接口。
public interface ISubtractable<T> {
    static abstract T Subtract(T a, T b);
}

/// 乘法接口。
public interface IMultiplicable<T> {
    static abstract T Multiply(T a, T b);
}

/// 除法接口。
public interface IDivisible<T> {
    static abstract T Divide(T a, T b);
}
