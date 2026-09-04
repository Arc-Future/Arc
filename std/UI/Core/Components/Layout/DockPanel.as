// RFC 037 D2.1 / RFC 037 D1: Arc.UI.Components.Layout — DockPanel 停靠布局。
//
// DockPanel 是停靠布局容器，子元素按 Top/Bottom/Left/Right/Fill 方向停靠。
//
// WPF 同构层级对照：
//   WPF: FrameworkElement → Panel → DockPanel
//   Arc:  FrameworkElement → Panel → DockPanel
//
// **冲突处理（RFC 037 D1 WPF 同构）**：
//   - Background 已由 Panel 声明——DockPanel 不重复声明，使用继承版本
//   - DockPanel 保留特有 DP：LastChildFill
//   - DockPanel 保留特有：Dock 附加属性（字符串占位）
//
// **附加属性占位说明**：
//   Arc 当前不支持 DependencyProperty.RegisterAttached——M3+ 升级后切换。
//   当前以 public static string 字段形式声明附加属性名占位。
//
// RFC 037 D1 WPF 同构编程模型：
//   每个公共属性仅由两件套驱动：
//     1. 静态 DependencyProperty<T> 元数据（由 RegisterProperty<T> 工厂创建）
//     2. 属性 wrapper 调用 Element.GetValue<T>/SetValue<T>
//   Signal<T> 后端由 Element 基类内部维护，用户不感知。

namespace Arc.UI.Components.Layout;

using Arc.UI.Layout;

/// <summary>停靠布局容器，子元素按 Top/Bottom/Left/Right/Fill 方向停靠。Background 由 Panel 继承。</summary>
public class DockPanel : Panel {
    /// <summary>构造元素并绑定运行时类型身份（供动态依赖属性解析）。</summary>
    public DockPanel() {
        this.Type = typeof(DockPanel);
    }

    // ===== 静态依赖属性元数据（RFC 037 D1 WPF 同构）=====

    /// <summary>LastChildFill 属性元数据——最后一个子元素是否填满剩余空间，默认 true。</summary>
    public static DependencyProperty<bool> LastChildFillProperty =
        RegisterProperty<bool>(nameof(LastChildFill), typeof(DockPanel), true);

    // ===== 公共属性 wrapper：委托 Element.GetValue<T>/SetValue<T> =====

    /// <summary>最后一个子元素是否填满剩余空间（默认 true）。</summary>
    public bool LastChildFill {
        get { return this.GetValue<bool>(LastChildFillProperty); }
        set { this.SetValue<bool>(LastChildFillProperty, value); }
    }

    // ===== 附加属性占位（M3+ 升级 RegisterAttached）=====

    /// <summary>DockPanel.Dock 附加属性（"Top"/"Bottom"/"Left"/"Right"/"Fill"）。</summary>
    public static string DockProperty = "DockPanel.Dock";

    protected override LayoutSize MeasureOverride(LayoutSize availableSize) {
        return DockLayout.Measure(this, availableSize);
    }

    protected override void ArrangeOverride(LayoutSize finalSize) {
        DockLayout.Arrange(this, finalSize);
    }
}
