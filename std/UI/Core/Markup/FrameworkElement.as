// RFC 037 D2.1 / RFC 037 D6: Arc.UI —— FrameworkElement 框架元素基类。
//
// FrameworkElement 是 WPF 同构层级中的「框架层元素」——在 Element
// （依赖属性存储宿主）之上扩展布局、样式、资源、数据上下文等
// 「框架级」语义。所有 .arml 标签直接对应的元素均派生自 FrameworkElement
// 或其子类（Control/Panel/Shape/ContentControl）。
//
// WPF 同构层级对照：
//   WPF: DispatcherObject → DependencyObject → Visual → UIElement
//           → FrameworkElement → Control → ContentControl → Button/Window/...
//   Arc:  Element (= DependencyObject + Visual + UIElement 合一)
//           → FrameworkElement → Control → ContentControl → Button/Window/...
//   Arc 简化原因：Arc 无 Dispatcher 同步上下文需求；Visual 与 UIElement 区分
//   由 wgpu 内部处理（命中测试走 GPU picking，无需 GDI+ 双缓冲）。
//
// FrameworkElement 承载的 WPF 同构 DP：
//   - Width/Height/MinWidth/MaxWidth/MinHeight/MaxHeight：布局尺寸约束
//   - Margin：外边距（Thickness）
//   - HorizontalAlignment/VerticalAlignment：对齐方式
//   - Style：样式引用（Style 对象）
//   - Resources：本地资源字典（ResourceDictionary）
//   - DataContext：数据上下文（升级为 DP，支持属性继承）
//   - Tag：用户自定义数据
//
// **DataContext 属性继承语义**：
//   WPF 中 DataContext 是 inherit DP——子元素未显式设置 DataContext 时
//   自动继承父元素的值。Arc 通过 GetValue 的 fallback 链实现：
//     DataContext getter 沿 Parent 链取最近祖先有效值 → ... → null
//   显式 SetValue(null) 写入本地槽，阻断向上继承；GetValue<object>(DataContextProperty)
//   为本地槽语义（未 SetValue 返 null，不沿 Parent 继承）。
//
// **命名空间归属**：本文件位于 std/UI/Markup/ 子目录，但归属到 `Arc.UI`
// 根命名空间（按 RFC 020 §3.2 + 命名空间分层原则：基类放根命名空间，
// 派生实现在子命名空间）。FrameworkElement 是 Control/Panel/Shape 等基类
// 的父类，必须在 `Arc.UI` 根命名空间，避免派生类需要同时 using 多个
// 子命名空间的反向引用反模式。

namespace Arc.UI;

using Arc.Collections;
using Arc.UI.Layout;

/// <summary>
/// 框架元素基类——扩展 Element 添加布局、样式、资源、数据上下文等。
/// WPF FrameworkElement 同构 DP。
/// </summary>
public class FrameworkElement : Element {
    /// <summary>构造元素并绑定运行时类型身份（供动态依赖属性解析）。</summary>
    public FrameworkElement() {
        this.Type = typeof(FrameworkElement);
    }

    // ===== 静态依赖属性元数据（RFC 051 D1 WPF 同构）=====

    /// <summary>Width 属性元数据——元素宽度（CSS 像素，NaN=自动）。</summary>
    public static DependencyProperty<double> WidthProperty =
        RegisterProperty<double>(nameof(Width), typeof(FrameworkElement), 0.0);

    /// <summary>Height 属性元数据——元素高度（CSS 像素，NaN=自动）。</summary>
    public static DependencyProperty<double> HeightProperty =
        RegisterProperty<double>(nameof(Height), typeof(FrameworkElement), 0.0);

    /// <summary>MinWidth 属性元数据——最小宽度约束。</summary>
    public static DependencyProperty<double> MinWidthProperty =
        RegisterProperty<double>(nameof(MinWidth), typeof(FrameworkElement), 0.0);

    /// <summary>MaxWidth 属性元数据——最大宽度约束。</summary>
    public static DependencyProperty<double> MaxWidthProperty =
        RegisterProperty<double>(nameof(MaxWidth), typeof(FrameworkElement), 0.0);

    /// <summary>MinHeight 属性元数据——最小高度约束。</summary>
    public static DependencyProperty<double> MinHeightProperty =
        RegisterProperty<double>(nameof(MinHeight), typeof(FrameworkElement), 0.0);

    /// <summary>MaxHeight 属性元数据——最大高度约束。</summary>
    public static DependencyProperty<double> MaxHeightProperty =
        RegisterProperty<double>(nameof(MaxHeight), typeof(FrameworkElement), 0.0);

    /// <summary>Margin 属性元数据——外边距（逗号分隔字符串 "l,t,r,b"）。</summary>
    /// <remarks>未来升级为 Thickness 类型 DP（待 Layout/Thickness 升级为 DP 友好）。</remarks>
    public static DependencyProperty<string> MarginProperty =
        RegisterProperty<string>(nameof(Margin), typeof(FrameworkElement), "0,0,0,0");

    /// <summary>HorizontalAlignment 属性元数据——水平对齐。</summary>
    public static DependencyProperty<HorizontalAlignment> HorizontalAlignmentProperty =
        RegisterProperty<HorizontalAlignment>(nameof(HorizontalAlignment), typeof(FrameworkElement), HorizontalAlignment.Stretch);

    /// <summary>VerticalAlignment 属性元数据——垂直对齐。</summary>
    public static DependencyProperty<VerticalAlignment> VerticalAlignmentProperty =
        RegisterProperty<VerticalAlignment>(nameof(VerticalAlignment), typeof(FrameworkElement), VerticalAlignment.Stretch);

    /// <summary>Style 属性元数据——引用的 Style 对象。</summary>
    public static DependencyProperty<object> StyleProperty =
        RegisterProperty<object>(nameof(Style), typeof(FrameworkElement), null);

    /// <summary>Resources 属性元数据——本地资源字典。</summary>
    public static DependencyProperty<object> ResourcesProperty =
        RegisterProperty<object>(nameof(Resources), typeof(FrameworkElement), null);

    /// <summary>Tag 属性元数据——用户自定义数据。</summary>
    public static DependencyProperty<object> TagProperty =
        RegisterProperty<object>(nameof(Tag), typeof(FrameworkElement), null);

    // ===== 公共属性 wrapper：委托 Element.GetValue<T>/SetValue<T> =====

    /// <summary>元素宽度（CSS 像素，NaN 表示自动）。</summary>
    public double Width {
        get { return this.GetValue<double>(WidthProperty); }
        set { this.SetValue<double>(WidthProperty, value); }
    }

    /// <summary>元素高度（CSS 像素，NaN 表示自动）。</summary>
    public double Height {
        get { return this.GetValue<double>(HeightProperty); }
        set { this.SetValue<double>(HeightProperty, value); }
    }

    /// <summary>最小宽度约束。</summary>
    public double MinWidth {
        get { return this.GetValue<double>(MinWidthProperty); }
        set { this.SetValue<double>(MinWidthProperty, value); }
    }

    /// <summary>最大宽度约束。</summary>
    public double MaxWidth {
        get { return this.GetValue<double>(MaxWidthProperty); }
        set { this.SetValue<double>(MaxWidthProperty, value); }
    }

    /// <summary>最小高度约束。</summary>
    public double MinHeight {
        get { return this.GetValue<double>(MinHeightProperty); }
        set { this.SetValue<double>(MinHeightProperty, value); }
    }

    /// <summary>最大高度约束。</summary>
    public double MaxHeight {
        get { return this.GetValue<double>(MaxHeightProperty); }
        set { this.SetValue<double>(MaxHeightProperty, value); }
    }

    /// <summary>外边距（逗号分隔字符串 "l,t,r,b"）。</summary>
    public string Margin {
        get { return this.GetValue<string>(MarginProperty); }
        set { this.SetValue<string>(MarginProperty, value); }
    }

    /// <summary>水平对齐方式：Left/Center/Right/Stretch。</summary>
    public HorizontalAlignment HorizontalAlignment {
        get { return this.GetValue<HorizontalAlignment>(HorizontalAlignmentProperty); }
        set { this.SetValue<HorizontalAlignment>(HorizontalAlignmentProperty, value); }
    }

    /// <summary>垂直对齐方式：Top/Center/Bottom/Stretch。</summary>
    public VerticalAlignment VerticalAlignment {
        get { return this.GetValue<VerticalAlignment>(VerticalAlignmentProperty); }
        set { this.SetValue<VerticalAlignment>(VerticalAlignmentProperty, value); }
    }

    /// <summary>引用的 Style 对象。</summary>
    public object Style {
        get { return this.GetValue<object>(StyleProperty); }
        set { this.SetValue<object>(StyleProperty, value); }
    }

    /// <summary>鏈湴璧勬簮瀛楀吀銆</summary>
    public object Resources {
        get { return this.GetValue<object>(ResourcesProperty); }
        set { this.SetValue<object>(ResourcesProperty, value); }
    }

    /// <summary>用户自定义数据（任意类型）。</summary>
    public object Tag {
        get { return this.GetValue<object>(TagProperty); }
        set { this.SetValue<object>(TagProperty, value); }
    }

    // ============================================================
    // 布局系统（RFC 026 D5 元组替代方案）：
    //
    // Arc 不支持元组返回 `(double, double)`，不支持 struct 值类型。
    // 替代方案：layoutSize class 封装 Width/Height 对；
    // Measure/Arrange 方法返回 void，尾尺存入 DesiredSize/RenderSize 字段。
    // ============================================================

    /// <summary>Measure 结果：元素期望的尺寸。</summary>
    public LayoutSize DesiredSize;

    /// <summary>Arrange 结果：元素最终渲染尺寸。</summary>
    public LayoutSize RenderSize;

    /// <summary>Arrange 结果宽度（PlatformTreeSync 用，避免 LayoutSize 字段 codegen）。</summary>
    public double RenderWidth;

    /// <summary>Arrange 结果高度。</summary>
    public double RenderHeight;

    /// <summary>Arrange 结果：相对窗口根的绝对 X（ArrangeChild 写入；PlatformTreeSync → LayoutX）。</summary>
    public double LayoutX;

    /// <summary>Arrange 结果：相对窗口根的绝对 Y（ArrangeChild 写入；PlatformTreeSync → LayoutY）。</summary>
    public double LayoutY;

    /// <summary>是否已完成 Measure（Arrange 前未 Measure 时安全降级）。</summary>
    public bool IsMeasured;

    /// <summary>
    /// 测量阶段：给定可用空间，计算期望尺寸并写入 DesiredSize。
    /// </summary>
    public virtual void Measure(LayoutSize availableSize) {
        LayoutSize available = LayoutHelper.SanitizeSize(availableSize);
        Thickness margin = LayoutHelper.GetMargin(this);
        LayoutSize innerAvailable = LayoutHelper.Deflate(available, margin);

        LayoutSize innerDesired = this.MeasureOverride(innerAvailable);
        innerDesired = LayoutHelper.ApplyMinMax(this, innerDesired);

        double iw = innerDesired.Width;
        double ih = innerDesired.Height;
        if (this.Width > 0.0) {
            iw = this.Width;
        }
        if (this.Height > 0.0) {
            ih = this.Height;
        }
        innerDesired = LayoutHelper.ApplyMinMax(this, new LayoutSize(iw, ih));

        this.DesiredSize = LayoutHelper.Inflate(innerDesired, margin);
        this.IsMeasured = true;
    }

    protected virtual LayoutSize MeasureOverride(LayoutSize availableSize) {
        return LayoutHelper.ComputeConstraintSize(this, availableSize);
    }

    /// <summary>
    /// 排列阶段：给定最终尺寸，设置 RenderSize 并递归排列子元素。
    /// </summary>
    public virtual void Arrange(LayoutSize finalSize) {
        double sizeW = LayoutHelper.Sanitize(finalSize.Width);
        double sizeH = LayoutHelper.Sanitize(finalSize.Height);
        LayoutSize size = new LayoutSize(sizeW, sizeH);
        Thickness margin = LayoutHelper.GetMargin(this);
        LayoutSize innerFinal = LayoutHelper.Deflate(size, margin);

        if (!this.IsMeasured) {
            this.Measure(new LayoutSize(sizeW, sizeH));
        }

        this.ArrangeOverride(innerFinal);
        this.RenderSize = new LayoutSize(sizeW, sizeH);
        this.RenderWidth = sizeW;
        this.RenderHeight = sizeH;
    }

    /// <summary>
    /// 默认叶节点：父级 <see cref="LayoutHelper.ArrangeChild"/> 已写入绝对 LayoutX/Y。
    /// 不得在此清零——否则整棵树塌到原点，渲染/命中全部叠在 (0,0)。
    /// </summary>
    protected virtual void ArrangeOverride(LayoutSize finalSize) {
    }
}
