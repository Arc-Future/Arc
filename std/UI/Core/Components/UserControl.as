// RFC 037 D2.1 + RFC 037 D1: Arc.UI.Components — UserControl 元素。
//
// UserControl 允许开发者将 .arml + .arml.as 组合封装为可复用控件（D2.1）。
//
// WPF 同构层级对照：
//   WPF: ContentControl → UserControl
//   Arc:  ContentControl → UserControl
//
// **冲突处理（RFC 037 D1 WPF 同构）**：
//   - Content 已由 ContentControl 声明——UserControl 不重复声明，使用继承版本
//   - UserControl 当前为空类——仅作类型标识，所有 DP 由基类继承
//
// RFC 037 D1 WPF 同构编程模型：
//   每个公共属性仅由两件套驱动：
//     1. 静态 DependencyProperty<T> 元数据（由 RegisterProperty<T> 工厂创建）
//     2. 属性 wrapper 调用 Element.GetValue<T>/SetValue<T>
//   Signal<T> 后端由 Element 基类内部维护，用户不感知。
//
// 实现状态：
//   - M1: 骨架声明 + RFC 037 DP 化
//   - M3: 由 .arml.as partial class 派生具体类型

namespace Arc.UI.Components;

/// <summary>
/// 用户自定义控件，封装 .arml + .arml.as 组合为可复用组件。
/// Content 等通用属性由 ContentControl 继承。
/// </summary>
public class UserControl : ContentControl {
    /// <summary>构造元素并绑定运行时类型身份（供动态依赖属性解析）。</summary>
    public UserControl() {
        this.Type = typeof(UserControl);
        // 容器型非 Tab 停靠（InputElement 默认的显式豁免；内容参与 Tab 循环）。
        this.IsTabStop = false;
    }

    // 当前为空类——仅作类型标识。所有 DP（Content/ContentTemplate/Padding 等）
    // 由 ContentControl 基类继承。后续 RFC 可在此添加用户控件特有语义。
}
