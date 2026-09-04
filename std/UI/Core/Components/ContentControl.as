// RFC 037 D2.1 / RFC 037 D1/D2: Arc.UI.Components — ContentControl 内容控件基类。
//
// ContentControl 是拥有单一 Content 的控件基类——Button/CheckBox/
// Window/UserControl/Page 等的父类。在 InputElement 之上扩展 Content DP
// 与 ContentTemplate DP（继承输入共性：焦点默认/键盘消费/默认激活——
// Button/CheckBox 链自动获得，容器型派生 Window/UserControl/Page 以
// IsTabStop=false 显式声明非停靠）。
//
// WPF 同构层级对照：
//   WPF: Control → ContentControl → Button/CheckBox/Window/UserControl/Page/...
//   Arc:  Control → InputElement → ContentControl → Button/CheckBox/Window/UserControl/Page/...
//
// RFC 037 D2 改造：Content 字段类型从 `object` 改为 `Content` variant
// （RFC 037 标签联合）。零装箱，类型安全。用户代码无需感知 variant 存在——
// typeck 自动隐式构造（RFC 037 §D9）：
//   button.Content = "Click";      // → Content.Text("Click")
//   button.Content = new Image();  // → Content.Element(image)
//
// **命名空间归属**：本文件位于 std/UI/Components/ 子目录，归属到
// `Arc.UI.Components` 命名空间。ContentControl 是「容器型控件」的基类，
// 与 Button/CheckBox 等交互控件同处一层。

namespace Arc.UI.Components;

using Arc.UI;

/// <summary>
/// 拥有单一 Content 的控件基类——所有容器型控件（Button/CheckBox/
/// Window/UserControl/Page/...）的父类。
/// </summary>
public class ContentControl : InputElement {
    /// <summary>构造元素并绑定运行时类型身份（供动态依赖属性解析）。</summary>
    public ContentControl() {
        this.Type = typeof(ContentControl);
        this.DataContent = null;
        this.DataTypeName = "";
    }

    // ===== 静态依赖属性元数据（RFC 037 D1 WPF 同构）=====

    /// <summary>Content 属性元数据——控件内容（Content variant：文本/Element/绑定/资源）。</summary>
    public static DependencyProperty<Content> ContentProperty =
        RegisterProperty<Content>(nameof(Content), typeof(ContentControl), Content.None);

    /// <summary>ContentTemplate 属性元数据——内容数据模板（DataTemplate）。</summary>
    public static DependencyProperty<object> ContentTemplateProperty =
        RegisterProperty<object>(nameof(ContentTemplate), typeof(ContentControl), null);

    /// <summary>ContentStringFormat 属性元数据——内容字符串格式化模板。</summary>
    public static DependencyProperty<string> ContentStringFormatProperty =
        RegisterProperty<string>(nameof(ContentStringFormat), typeof(ContentControl), null);

    /// <summary>ContentDirection 属性元数据——内容流方向。</summary>
    public static DependencyProperty<string> ContentDirectionProperty =
        RegisterProperty<string>(nameof(ContentDirection), typeof(ContentControl), "LeftToRight");

    /// <summary>HorizontalContentAlignment 属性元数据——内容水平对齐。</summary>
    public static DependencyProperty<HorizontalAlignment> HorizontalContentAlignmentProperty =
        RegisterProperty<HorizontalAlignment>(nameof(HorizontalContentAlignment), typeof(ContentControl), HorizontalAlignment.Stretch);

    /// <summary>VerticalContentAlignment 属性元数据——内容垂直对齐。</summary>
    public static DependencyProperty<VerticalAlignment> VerticalContentAlignmentProperty =
        RegisterProperty<VerticalAlignment>(nameof(VerticalContentAlignment), typeof(ContentControl), VerticalAlignment.Stretch);

    /// <summary>Padding 属性元数据——内边距（逗号分隔字符串 "l,t,r,b"）。</summary>
    public static DependencyProperty<string> PaddingProperty =
        RegisterProperty<string>(nameof(Padding), typeof(ContentControl), "0,0,0,0");

    // ===== 公共属性 wrapper：委托 Element.GetValue<T>/SetValue<T> =====

    /// <summary>控件内容（Content variant：文本/Element/绑定/资源）。</summary>
    public Content Content {
        get { return this.GetValue<Content>(ContentProperty); }
        set { this.SetValue<Content>(ContentProperty, value); }
    }

    /// <summary>内容数据模板（DataTemplate）。</summary>
    public object ContentTemplate {
        get { return this.GetValue<object>(ContentTemplateProperty); }
        set { this.SetValue<object>(ContentTemplateProperty, value); }
    }

    /// <summary>内容字符串格式化模板（如 "{0:N2}"）。</summary>
    public string ContentStringFormat {
        get { return this.GetValue<string>(ContentStringFormatProperty); }
        set { this.SetValue<string>(ContentStringFormatProperty, value); }
    }

    /// <summary>内容流方向："LeftToRight" / "RightToLeft"。</summary>
    public string ContentDirection {
        get { return this.GetValue<string>(ContentDirectionProperty); }
        set { this.SetValue<string>(ContentDirectionProperty, value); }
    }

    /// <summary>内容水平对齐：Left/Center/Right/Stretch。</summary>
    public HorizontalAlignment HorizontalContentAlignment {
        get { return this.GetValue<HorizontalAlignment>(HorizontalContentAlignmentProperty); }
        set { this.SetValue<HorizontalAlignment>(HorizontalContentAlignmentProperty, value); }
    }

    /// <summary>内容垂直对齐：Top/Center/Bottom/Stretch。</summary>
    public VerticalAlignment VerticalContentAlignment {
        get { return this.GetValue<VerticalAlignment>(VerticalContentAlignmentProperty); }
        set { this.SetValue<VerticalAlignment>(VerticalContentAlignmentProperty, value); }
    }

    /// <summary>内边距（逗号分隔字符串 "l,t,r,b"）。</summary>
    public string Padding {
        get { return this.GetValue<string>(PaddingProperty); }
        set { this.SetValue<string>(PaddingProperty, value); }
    }

    /// <summary>
    /// M3.6 平台同步镜像：codegen 从 ARML Content 属性填充。
    /// 待 Signal&lt;Content&gt; 构造/codegen 就绪后可改读 Content DP。
    /// </summary>
    public string MirrorContent;

    /// <summary>
    /// 数据内容载荷（WPF Content 非元素数据对象对齐）：Content variant 无
    /// object 数据分支且禁止扩分支（variant 布局变更触发 codegen 泛型 cast
    /// 错位），非 Element 数据对象经此独立字段承载（Setter.TemplateValue
    /// 同模式）；null = 未用数据路径，ContentPresenter 仅在非 null 时走
    /// DataTemplate 分派。
    /// </summary>
    public object DataContent;

    /// <summary>
    /// 数据内容类型名（隐式 DataTemplate 匹配键）：WPF 经反射取数据对象
    /// GetType().Name，Arc 运行时不支持对象类型反查（ArcBox 无 type_id），
    /// 由设置者显式标注，与 ResourceDictionary 注册的 DataTemplate.DataType
    /// 字符串键匹配；空串 = 不参与隐式匹配。
    /// </summary>
    public string DataTypeName;
}
