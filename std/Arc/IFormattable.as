namespace Arc;

/// <summary>通用字符串渲染接口（RFC 016 D4.1）。</summary>
///
/// 替代 RFC 016 v1 `Object.ToString()` 的装箱语义——为任意用户类提供统一、
/// 零装箱、编译期可折叠的字符串渲染（日志/调试/展示等横切需求）。
/// 零装箱：泛型 `Format<T>(T)` 单态化后在编译期折叠；无法静态确定时经接口虚表分派。
///
/// **默认回退**（编译器内置展开）：类型实现 `IFormattable` → 调其 `ToString()`；
/// 否则默认渲染为 `typeof(T).Name`（对齐 C# 默认 `object.ToString()` = 类型名，
/// 但零装箱——`typeof(T)` 为编译期常量，无运行时分配）。
///
/// **分工边界**（RFC 002 单一惯用法）：
/// - 基元类型 → 内置 `rt_*_to_string` ABI（不实现本接口）
/// - 数值泛型运算链 → `INumber<T>.ToString()`（与 `IFormattable` 不双轨）
/// - 显式类型转换门面 → `Convert.ToString(x)`（标量互转）
/// - 任意对象通用渲染 → 本接口
public interface IFormattable {
    /// <summary>返回当前对象的字符串表示（无格式）。文化无关。</summary>
    string ToString();

    /// <summary>按格式串与格式文化返回当前对象的字符串表示（如 "D" / "F2" / "X"）。</summary>
    /// <param name="format">格式串；空串或不支持时回退到无格式渲染。</param>
    /// <param name="provider">格式提供程序（CultureInfo / NumberFormatInfo / DateTimeFormatInfo）；null 时用 CultureInfo.CurrentCulture。</param>
    /// <remarks>
    /// 本重载依赖「文化感知格式化」前置条件（RFC 006 §12.5：IFormatProvider +
    /// NumberFormatInfo/DateTimeFormatInfo + CultureInfo 格式数据）。
    /// 前置条件已随 RFC 027 M5 全球化增强验收达成（culture_format_e2e），门禁解除；
    /// 数值基元的有参 ToString(format, provider) 走内置 `rt_*_to_string_fmt_p` ABI，
    /// 用户类型实现本接口即获得文化感知渲染。
    /// </remarks>
    string ToString(string format, IFormatProvider provider);
}