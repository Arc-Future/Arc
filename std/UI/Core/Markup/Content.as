// RFC 037 D2 / RFC 037：Arc.UI — Content variant 内容承载类型。
//
// Content 字段承载类型——.arml 中 Content 可为字面字符串、子元素、
// 绑定表达式或资源引用。case 集合在框架层封闭，用户无需感知 variant
// 存在（typeck 自动隐式构造，见 RFC 037 §D9）。
//
// **命名空间归属**：本文件位于 std/UI/Markup/ 子目录，归属到 `Arc.UI` 根命名空间。
// 按命名空间分层原则——基础类型放根命名空间，派生实现在子命名空间。
// Content 是所有 ContentControl 派生类的 Content 属性类型，与 Element/
// Binding 同层，必须在 `Arc.UI` 根命名空间。
//
// **零装箱架构**：variant 为栈分配标签联合（RFC 037），替代 WPF 的
// `object` 字段——消除堆分配与运行时类型检查开销。
//
// 使用模式（隐式构造，typeck 自动重写）：
//   button.Content = "Click";           // → Content.Text("Click")
//   button.Content = new Image();       // → Content.Element(image)
//   button.Content = "{Binding name}";  // → Content.Binding(...)
//
// 显式构造（必要时）：
//   Content c = Content.Text("Hello");
//   Content c = Content.Element(new Button());

namespace Arc.UI;

/// <summary>
/// Content 字段承载类型——.arml 中 Content 可为字面字符串、子元素、
/// 绑定表达式或资源引用。case 集合在框架层封闭。
/// </summary>
public variant Content {
    /// <summary>无内容——ContentControl 的默认状态。</summary>
    | None
    /// <summary>字面文本内容（string → Content.Text 隐式构造）。</summary>
    | Text of string
    /// <summary>子元素内容（Element 子类 → Content.Element 隐式构造）。</summary>
    | Element of Element
    /// <summary>绑定表达式（Binding → Content.Binding 隐式构造）。</summary>
    | Binding of Binding
    /// <summary>资源引用 key（StaticResource；应用期按活动主题解析）。</summary>
    | Resource of string
}
