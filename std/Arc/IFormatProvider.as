// RFC 034 M5: 文化感知格式化 — 格式提供程序接口 IFormatProvider。
//
// 对标 C# System.IFormatProvider（根命名空间，与 IFormattable 同层）。
//
// 定位：按类型取格式模板的抽象——格式化方（数值/日期）调用 GetFormat 获取
// 对应文化下的 NumberFormatInfo / DateTimeFormatInfo。CultureInfo 实现本接口，
// 使其可直接作为 provider 传入 `IFormattable.ToString(format, provider)`。

namespace Arc;

using Arc.Reflection;

/// <summary>
/// 格式提供程序接口——按格式类型返回对应文化下的格式模板。
///
/// 实现方：<see cref="CultureInfo"/>（返回其 NumberFormat / DateTimeFormat）。
/// 消费方：数值 / 日期格式化，及 <c>IFormattable.ToString(format, provider)</c>。
/// </summary>
public interface IFormatProvider {
    /// <summary>按类型获取格式模板。</summary>
    /// <param name="formatType">请求的格式类型（NumberFormatInfo 或 DateTimeFormatInfo 的类型）。</param>
    /// <returns>对应格式模板；formatType 为 null 或不受支持时返回 null。</returns>
    object GetFormat(Type formatType);
}