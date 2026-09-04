// RFC 037 D2.1 / D5.3 / RFC 037 D1: Arc.UI.Components.Layout — Canvas 绝对定位布局。
//
// Canvas 是绝对定位布局容器，子元素通过 Left/Top/Right/Bottom 附加属性定位。
//
// WPF 同构层级对照：
//   WPF: FrameworkElement → Panel → Canvas
//   Arc:  FrameworkElement → Panel → Canvas
//
// **冲突处理（RFC 037 D1 WPF 同构）**：
//   - Background 已由 Panel 声明——Canvas 不重复声明，使用继承版本
//   - Canvas 保留特有：Left/Top/Right/Bottom 附加属性（字符串占位）
//
// **附加属性占位说明**：
//   Arc 当前不支持 DependencyProperty.RegisterAttached——M3+ 升级后切换。
//   当前以 public static string 字段形式声明附加属性名占位，
//   .arml parser 识别 `Canvas.Left="10"` 后通过 ElementSetString 设置。
//
// RFC 037 D1 WPF 同构编程模型：
//   每个公共属性仅由两件套驱动：
//     1. 静态 DependencyProperty<T> 元数据（由 RegisterProperty<T> 工厂创建）
//     2. 属性 wrapper 调用 Element.GetValue<T>/SetValue<T>
//   Signal<T> 后端由 Element 基类内部维护，用户不感知。

namespace Arc.UI.Components.Layout;

using Arc.UI.Layout;

/// <summary>
/// 绝对定位布局容器，子元素通过 Left/Top/Right/Bottom 附加属性定位。
/// Background 由 Panel 继承。
/// </summary>
public class Canvas : Panel {
    /// <summary>构造元素并绑定运行时类型身份（供动态依赖属性解析）。</summary>
    public Canvas() {
        this.Type = typeof(Canvas);
    }

    // ===== 附加属性占位（M3+ 升级 RegisterAttached）=====
    //
    // 当前为字符串常量——表示附加属性名，供 .arml parser 识别。
    // M3+ 升级为 DependencyProperty<double>.RegisterAttached("Canvas.Left", ...)。

    /// <summary>Canvas.Left 附加属性（M3+ 升级 DependencyProperty.RegisterAttached）。</summary>
    public static string LeftProperty = "Canvas.Left";

    /// <summary>Canvas.Top 附加属性。</summary>
    public static string TopProperty = "Canvas.Top";

    /// <summary>Canvas.Right 附加属性。</summary>
    public static string RightProperty = "Canvas.Right";

    /// <summary>Canvas.Bottom 附加属性。</summary>
    public static string BottomProperty = "Canvas.Bottom";

    protected override LayoutSize MeasureOverride(LayoutSize availableSize) {
        return CanvasLayout.Measure(this, availableSize);
    }

    protected override void ArrangeOverride(LayoutSize finalSize) {
        CanvasLayout.Arrange(this, finalSize);
    }
}
