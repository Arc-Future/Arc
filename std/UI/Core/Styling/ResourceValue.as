// RFC 037 D4 / 语言核心 variant（见 004 类型系统）：Arc.UI.Styling — ResourceValue variant。
//
// 资源值类型——ResourceDictionary 中值的类型化承载。替代 WPF 的
// `object` Value——使用语言核心 variant 原生标签联合（见 004 类型系统），零装箱。
//
// **命名空间归属**：本文件位于 std/UI/Styling/ 子目录，归属到
// `Arc.UI.Styling` 命名空间。
//
// 使用模式（隐式构造，typeck 自动重写）：
//   dict.Add("AccentColor", "#FF0000");   // → ResourceValue.String("#FF0000")
//   dict.Add("FontSize", 14.0);           // → ResourceValue.Number(14.0)
//   dict.Add("IsEnabled", true);          // → ResourceValue.Boolean(true)
//
// 显式构造：
//   ResourceValue v = ResourceValue.Style(myStyle);
//   ResourceValue v = ResourceValue.Template(ctrlTemplate);

namespace Arc.UI.Styling;

using Arc.UI;
using Arc.UI.Media;

/// <summary>
/// 资源值类型——ResourceDictionary 中值的类型化承载。
/// 使用语言核心 variant 原生标签联合（见 004 类型系统），零装箱。
/// </summary>
public variant ResourceValue {
    /// <summary>字符串资源（字体名、URL 等）。</summary>
    | String of string
    /// <summary>画刷资源（纯色/渐变，类型化承载颜色家族；见 Arc.UI.Media）。</summary>
    | Brush of Brush
    /// <summary>数值资源（尺寸、不透明度等）。</summary>
    | Number of double
    /// <summary>布尔资源（可见性开关等）。</summary>
    | Boolean of bool
    /// <summary>元素资源（图标、矢量图形等）。</summary>
    | Element of Element
    /// <summary>样式资源。</summary>
    | Style of Style
    /// <summary>控件模板资源。</summary>
    | Template of ControlTemplate
}
