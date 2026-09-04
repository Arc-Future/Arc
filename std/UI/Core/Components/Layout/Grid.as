// RFC 037 D2.1 / D5.2 / RFC 037 D1: Arc.UI.Components.Layout — Grid 网格布局容器。
//
// Grid 是网格布局容器，按行列定义排列子元素。
//
// WPF 同构层级对照：
//   WPF: FrameworkElement → Panel → Grid
//   Arc:  FrameworkElement → Panel → Grid
//
// **冲突处理（RFC 037 D1 WPF 同构）**：
//   - Background 已由 Panel 声明——Grid 不重复声明，使用继承版本
//   - Grid 保留特有 DP：ColumnSpacing/RowSpacing
//   - Grid 附加属性：Row/Column（RFC 037 typed `DependencyProperty<int>` +
//     静态访问器 GetRow/SetRow/GetColumn/SetColumn；不引入 RegisterAttached）
//   - Grid 保留特有字段：ColumnDefinitions/RowDefinitions（List<object>）
//     （非 DP，由 codegen 在 InitializeComponent 中构造 List 后赋值）
//
// RFC 037 D1 WPF 同构编程模型：
//   每个公共属性仅由两件套驱动：
//     1. 静态 DependencyProperty<T> 元数据（由 RegisterProperty<T> 工厂创建）
//     2. 属性 wrapper 调用 Element.GetValue<T>/SetValue<T>
//   Signal<T> 后端由 Element 基类内部维护，用户不感知。

namespace Arc.UI.Components.Layout;

using Arc.Collections;
using Arc.UI.Layout;

/// <summary>网格布局容器，按行列定义排列子元素。Background 由 Panel 继承。</summary>
public class Grid : Panel {
    /// <summary>构造元素并绑定运行时类型身份（供动态依赖属性解析）。</summary>
    public Grid() {
        this.Type = typeof(Grid);
    }

    // ===== 静态依赖属性元数据（RFC 037 D1 WPF 同构）=====

    /// <summary>ColumnSpacing 属性元数据——列间距，默认 0.0。</summary>
    public static DependencyProperty<double> ColumnSpacingProperty =
        RegisterProperty<double>(nameof(ColumnSpacing), typeof(Grid), 0.0);

    /// <summary>RowSpacing 属性元数据——行间距，默认 0.0。</summary>
    public static DependencyProperty<double> RowSpacingProperty =
        RegisterProperty<double>(nameof(RowSpacing), typeof(Grid), 0.0);

    // ===== 附加属性（RFC 037：Grid.Row / Grid.Column typed · 不引入 RegisterAttached）=====
    //
    // WPF 心智对齐：宿主静态 DP 元数据 + 宿主静态 typed 访问器（GetRow/SetRow），
    // 目标元素上存普通 DP 槽。`Row`/`Column` 非 Grid 类成员，故元数据 Name 用
    // 字面量（nameof 无法解析非成员名）；`DockPanel.Dock`/`Canvas.Left` 仍为
    // string 键占位，不随本 RFC typed 化（各自独立后续 RFC）。

    /// <summary>Grid.Row 附加属性元数据——子元素所在行，默认 0。</summary>
    public static DependencyProperty<int> RowProperty =
        RegisterProperty<int>("Row", typeof(Grid), 0);

    /// <summary>Grid.Column 附加属性元数据——子元素所在列，默认 0。</summary>
    public static DependencyProperty<int> ColumnProperty =
        RegisterProperty<int>("Column", typeof(Grid), 0);

    /// <summary>读取子元素所在行（Grid.Row 附加属性）。</summary>
    public static int GetRow(Element elem) {
        if (elem == null) {
            return 0;
        }
        return elem.GetValue<int>(RowProperty);
    }

    /// <summary>设置子元素所在行（Grid.Row 附加属性）。</summary>
    public static void SetRow(Element elem, int value) {
        if (elem == null) {
            return;
        }
        elem.SetValue<int>(RowProperty, value);
    }

    /// <summary>读取子元素所在列（Grid.Column 附加属性）。</summary>
    public static int GetColumn(Element elem) {
        if (elem == null) {
            return 0;
        }
        return elem.GetValue<int>(ColumnProperty);
    }

    /// <summary>设置子元素所在列（Grid.Column 附加属性）。</summary>
    public static void SetColumn(Element elem, int value) {
        if (elem == null) {
            return;
        }
        elem.SetValue<int>(ColumnProperty, value);
    }

    // ===== 公共属性 wrapper：委托 Element.GetValue<T>/SetValue<T> =====

    /// <summary>列间距（像素）。</summary>
    public double ColumnSpacing {
        get { return this.GetValue<double>(ColumnSpacingProperty); }
        set { this.SetValue<double>(ColumnSpacingProperty, value); }
    }

    /// <summary>行间距（像素）。</summary>
    public double RowSpacing {
        get { return this.GetValue<double>(RowSpacingProperty); }
        set { this.SetValue<double>(RowSpacingProperty, value); }
    }

    // ===== 非 DP 字段：列/行定义集合 =====
    //
    // ColumnDefinitions/RowDefinitions 当前为 List<object> 字段（非 DP）——
    // 由 codegen 在 InitializeComponent 中构造 List 后赋值。
    // M3+ 升级为 DependencyProperty<List<object>> 后可参与绑定系统。

    /// <summary>列定义集合（每个定义含 Width/MinWidth/MaxWidth）。</summary>
    public List<object> ColumnDefinitions;

    /// <summary>行定义集合（每个定义含 Height/MinHeight/MaxHeight）。</summary>
    public List<object> RowDefinitions;

    protected override LayoutSize MeasureOverride(LayoutSize availableSize) {
        return GridLayout.Measure(this, availableSize);
    }

    protected override void ArrangeOverride(LayoutSize finalSize) {
        GridLayout.Arrange(this, finalSize);
    }
}
