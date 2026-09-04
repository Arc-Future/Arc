// Arc.UI.Styling — SetterValue variant（语言核心和式类型，见 docs/rfc/004-type-system.md）。
//
// Setter 值类型——Style 中属性值的类型化承载。替代旧 struct + Tag 手动
// 联合体，改用语言核心 variant（栈分配标签联合，见 004 类型系统）。
//
// **命名空间归属**：本文件位于 std/UI/Styling/ 子目录，归属到
// `Arc.UI.Styling` 命名空间。SetterValue 是 Styling 子系统的内部类型。
//
// **零装箱架构**：variant 为栈分配标签联合，替代旧 struct 的 Tag + 字段
// 联合——语义更清晰，类型更安全，编译器原生支持 switch 模式匹配。
//
// 使用模式（隐式构造，typeck 自动重写）：
//   setter.Value = "Red";        // → SetterValue.String("Red")
//   setter.Value = 14.0;         // → SetterValue.Number(14.0)
//   setter.Value = true;         // → SetterValue.Boolean(true)
//
// 显式构造：
//   SetterValue v = SetterValue.String("Red");
//   SetterValue v = SetterValue.Number(14.0);

namespace Arc.UI.Styling;

using Arc.UI;

/// <summary>
/// Setter 值类型——Style 中属性值的类型化承载。
/// 替代旧 struct + Tag 手动联合体，使用语言核心 variant（见 004 类型系统）原生标签联合。
/// </summary>
public variant SetterValue {
    /// <summary>字符串值（如颜色名 "Red"、字体名 "Segoe UI"）。</summary>
    | String of string
    /// <summary>数值（如 FontSize=14.0、Width=100.0）。</summary>
    | Number of double
    /// <summary>布尔值（如 IsEnabled=true）。</summary>
    | Boolean of bool
    /// <summary>元素值（如 Setter 设置 Complex 内容）。</summary>
    | Element of Element
    /// <summary>绑定表达式。</summary>
    | Binding of Binding
    /// <summary>静态资源引用 key（应用期按当前活动主题解析；主题即资源，经 MergedDictionaries 并入解析链）。</summary>
    | StaticResource of string
    /// <summary>模板绑定路径。</summary>
    | TemplateBinding of string
}
