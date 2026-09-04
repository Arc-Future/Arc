// RFC 037 §10 AI 原生：IElementFactory — 运行时元素创建工厂接口。
//
// LivePreviewHost 通过此接口从类型名创建 Element 实例——
// 解耦解析器与具体元素类型，允许自定义元素注册。

namespace Arc.UI.Markup;

using Arc.UI;

/// <summary>
/// 运行时元素工厂——按类型名创建 Element 实例。
/// </summary>
public interface IElementFactory {
    /// <summary>
    /// 按类型名创建 Element 实例。未知类型返回 null。
    /// </summary>
    /// <param name="typeName">元素类型名（如 "StackPanel"/"Button"/"TextBlock"）。</param>
    /// <returns>新创建的 Element 实例；类型未知返回 null。</returns>
    Element Create(string typeName);
}
