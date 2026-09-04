// RFC 037 D2.1 / RFC 037 D1 / RFC 037 M-VZ1: ItemsControl 基类。
//
// ItemsControl 是拥有项集合的控件基类（ListView 等的父类）。
// **RFC 037**：默认 VirtualizingStackPanel 项宿主；禁止全量 AddChild。
//
// 数据面单一惯用法（WPF 对标）：项集合来源唯一入口是 ItemsSource 属性
// （object 槽，判别物化为 ItemSourceView 数据源视图；null 清空）；呈现定制
// 唯一入口是 ItemTemplate（DataTemplate Instantiate/Recycle 委托对经
// ItemContainerGenerator 模板路径物化）。DisplayMemberPath 反射路径已撤除
// （RFC 037 无反射目标）：显示投影经视图工厂编译期锁定（From&lt;T&gt; display 委托 /
// EnumOptions DisplayName）。命令式 Set*Items 公开 API 已撤面（双轨禁令，
// RFC 001 单一惯用法）；派生控件读自身数据源经 protected View（如 ComboBox
// 下拉派生物化、Selector 选中项本体读取）。
//
// 数据面目标态（RFC 037）：string / List&lt;string&gt; 为便捷轨（显示即本体，活引用）；
// 强类型轨 From&lt;T&gt; 烘焙「object 本体 + string 投影」平行表（SelectedItem 本体化）；
// 动态轨 ObservableCollection&lt;string&gt; 订阅迁入视图。ItemsControl 只消费视图的
// object 管道（Count/ItemAt/DisplayAt）与变更表面，不再感知源类型——
// 模板路径收数据项本体（WPF DataContext 同构），默认路径收显示投影。
//
// 视图变更表面订阅（诚实标注）：用**静态方法组**注册（bare fn ptr，无捕获
// 闭包）——编译器对逃逸闭包的 ByRef 捕获存外层栈槽地址，闭包跨函数逃逸后槽位
// 悬垂 → UB（lambda 订阅实测偶发不触发/AV，2026-08-05）。静态方法组无 env 无
// 悬垂，路由经 `_activeViewHost`；**单活跃实例**约束（多实例并发订阅依赖编译器
// 逃逸闭包修复，属依赖项；与原 ObservableCollection 直订机制约束等价，非退化）。

namespace Arc.UI.Components;

using Arc.Collections;
using Arc.UI;
using Arc.UI.Components.Layout;
using Arc.UI.Layout;
using Arc.UI.Styling;

/// <summary>拥有项集合的控件基类。ItemsSource 绑定大列表时视口虚拟化（RFC 037）。</summary>
public class ItemsControl : Control {
    private VirtualizingStackPanel _itemsHost;
    private ItemSourceView _view;
    private int _viewToken;

    public ItemContainerGenerator ItemContainerGenerator;

    /// <summary>当前数据源视图（只读面）：派生控件读取自身数据源做派生物化
    /// （如 ComboBox 下拉列表）与选中项本体/投影读取（Count/ItemAt/DisplayAt）。
    /// null = 无源。</summary>
    protected ItemSourceView View {
        get { return _view; }
    }

    public ItemsControl() {
        this.Type = typeof(ItemsControl);
        this.TypeName = "ItemsControl";
        _itemsHost = new VirtualizingStackPanel();
        _itemsHost.TypeName = "VirtualizingStackPanel";
        _itemsHost.Orientation = Orientation.Vertical;
        this.AddChild(_itemsHost);
        ItemContainerGenerator = new ItemContainerGenerator(_itemsHost);
        _itemsHost.Generator = ItemContainerGenerator;
    }

    /// <summary>自管视口派生（DataGrid）入口：ownsItemsHost=false 跳过基类项宿主装配，
    /// 派生控件自管视口管线；基类数据面（ItemsSource/ItemTemplate/滚动偏移）随之不可用
    /// （各触点 _itemsHost null 守卫早退）。</summary>
    protected ItemsControl(bool ownsItemsHost) {
        this.Type = typeof(ItemsControl);
        this.TypeName = "ItemsControl";
        if (!ownsItemsHost) {
            return;
        }
        _itemsHost = new VirtualizingStackPanel();
        _itemsHost.TypeName = "VirtualizingStackPanel";
        _itemsHost.Orientation = Orientation.Vertical;
        this.AddChild(_itemsHost);
        ItemContainerGenerator = new ItemContainerGenerator(_itemsHost);
        _itemsHost.Generator = ItemContainerGenerator;
    }

    public static DependencyProperty<object> ItemsSourceProperty =
        RegisterProperty<object>(nameof(ItemsSource), typeof(ItemsControl), null);

    public static DependencyProperty<object> ItemTemplateProperty =
        RegisterProperty<object>(nameof(ItemTemplate), typeof(ItemsControl), null);

    public static DependencyProperty<object> ItemsPanelProperty =
        RegisterProperty<object>(nameof(ItemsPanel), typeof(ItemsControl), null);

    public static DependencyProperty<double> VerticalOffsetProperty =
        RegisterProperty<double>(nameof(VerticalOffset), typeof(ItemsControl), 0.0);

    public static DependencyProperty<double> ItemHeightProperty =
        RegisterProperty<double>(nameof(ItemHeight), typeof(ItemsControl), 0.0);

    /// <summary>
    /// 项集合来源（object 槽，WPF 对标唯一数据入口）。设置后按运行时类型判别
    /// 自动物化为数据源视图：`string` → 单项；`List&lt;string&gt;` → 静态列表
    /// （活引用）；`ObservableCollection&lt;string&gt;` → 集合级增量通道（RFC 037 M6，
    /// 订阅迁入视图）；null 或无法判别的源 → 清空。强类型源经派生面以
    /// ItemSourceView.From&lt;T&gt;（编译期显示投影）承载。呈现定制见 ItemTemplate。
    ///
    /// RFC 037 M3：object 槽内 string 值由编译器装箱（rt_string_box，带
    /// vtable→rt_typeinfo_string），使 `is string` / `is List&lt;string&gt;`
    /// 判别安全（此前裸 char* 读偏移 8 作 vtable 为运行期 UB，OOP 挂账）。
    /// </summary>
    public object ItemsSource {
        get { return this.GetValue<object>(ItemsSourceProperty); }
        set {
            this.SetValue<object>(ItemsSourceProperty, value);
            this.MaterializeFromItemsSource();
        }
    }

    /// <summary>按 object ItemsSource 的运行时类型判别并物化为数据源视图；null 与
    /// 未知类型统一走清空路径（WPF ItemsSource = null 语义，行为可预期）。</summary>
    private void MaterializeFromItemsSource() {
        object src = this.GetValue<object>(ItemsSourceProperty);
        if (src == null) {
            this.ClearItems();
            return;
        }
        if (src is string) {
            this.SetView(ItemSourceView.From((string)src));
        } else if (src is List<string>) {
            this.SetView(ItemSourceView.From((List<string>)src));
        } else if (src is ObservableCollection<string>) {
            this.SetView(ItemSourceView.From((ObservableCollection<string>)src));
        } else if (src is ItemSourceView) {
            this.SetView((ItemSourceView)src);
        } else {
            this.ClearItems();
        }
    }

    /// <summary>换绑数据源视图：释放旧视图、登记新视图表面订阅并同步物化管线
    /// （Generator.SetView 重置物化窗口）。null = 清空（WPF ItemsSource = null 语义）。</summary>
    private void SetView(ItemSourceView view) {
        this.ReleaseView();
        _view = view;
        // 方法组委托在函数作用域声明（MIR 块级 Let 走 AST 原样、丢 typeck 脱糖：
        // 块内 `Action<...> h = ItemsControl.X;` 会触发 lower_expr 未解析 ident panic，
        // 2026-08-05 实测；函数顶层走 TypedStmt 带脱糖后 lambda）。
        Action<CollectionChangedEventArgs<object>> handler = ItemsControl.OnViewChangedStatic;
        if (view != null) {
            _activeViewHost = this;
            _viewToken = view.OnChanged(handler);
        }
        if (_itemsHost == null) {
            return;
        }
        this.ItemContainerGenerator.SetView(view);
        this.RefreshItems();
    }

    /// <summary>清空项集合（释放视图、重置物化器、刷新为空）。</summary>
    private void ClearItems() {
        this.SetView(null);
    }

    /// <summary>释放当前视图：退订视图变更表面、解除视图与动态源的绑定
    /// （视图 Detach 退订源通道并清路由槽，防换绑后悬垂派发）、清静态路由槽。</summary>
    private void ReleaseView() {
        if (_view != null) {
            _view.Unsubscribe(_viewToken);
            _view.Detach();
            _view = null;
        }
        _viewToken = -1;
        if (_activeViewHost == this) {
            _activeViewHost = null;
        }
    }

    private static ItemsControl _activeViewHost;

    /// <summary>视图变更表面静态路由（bare fn ptr，无捕获）：定位当前活跃
    /// ItemsControl 实例派发（单活跃实例约束，见文件头）。</summary>
    private static void OnViewChangedStatic(CollectionChangedEventArgs<object> args) {
        ItemsControl host = _activeViewHost;
        if (host != null) {
            host.OnViewChanged(args);
        }
    }

    private void OnViewChanged(CollectionChangedEventArgs<object> args) {
        if (_itemsHost == null) {
            return;
        }
        _itemsHost.ApplyCollectionChange(args, this.CreateDefaultsText());
    }

    /// <summary>
    /// 项模板（object 槽）：DataTemplate 时经 ItemContainerGenerator 模板路径物化
    ///（Instantiate 新建 / Recycle 重绑委托对，WPF ItemTemplate 对齐；Recycle 收
    /// 数据项本体）；null 或其他值回退默认显示投影 → TextBlock 物化。
    /// </summary>
    public object ItemTemplate {
        get { return this.GetValue<object>(ItemTemplateProperty); }
        set {
            this.SetValue<object>(ItemTemplateProperty, value);
            if (_itemsHost == null) {
                return;
            }
            DataTemplate template = null;
            if (value is DataTemplate) {
                template = (DataTemplate)value;
            }
            this.ItemContainerGenerator.SetTemplate(template);
            this.RefreshItems();
        }
    }

    public object ItemsPanel {
        get { return this.GetValue<object>(ItemsPanelProperty); }
        set { this.SetValue<object>(ItemsPanelProperty, value); }
    }

    /// <summary>垂直滚动偏移（px）；ScrollView 外壳同步此值以驱动视口窗口。</summary>
    public double VerticalOffset {
        get { return this.GetValue<double>(VerticalOffsetProperty); }
        set {
            this.SetValue<double>(VerticalOffsetProperty, value);
            if (_itemsHost == null) {
                return;
            }
            _itemsHost.VerticalOffset = value;
            this.RefreshItems();
        }
    }

    /// <summary>等高项 stride（0 = 由 FontSize 估算）。</summary>
    public double ItemHeight {
        get { return this.GetValue<double>(ItemHeightProperty); }
        set {
            this.SetValue<double>(ItemHeightProperty, value);
            if (_itemsHost == null) {
                return;
            }
            _itemsHost.ItemHeight = value;
            this.RefreshItems();
        }
    }

    public Panel ItemsHost {
        get { return _itemsHost; }
    }

    /// <summary>算术内容总高（itemCount × stride；RFC 037 §4.1）。</summary>
    public double ContentExtentHeight {
        get {
            if (_itemsHost == null) {
                return 0.0;
            }
            return _itemsHost.ExtentHeight;
        }
    }

    public override void OnLoaded() {
        this.RefreshItems();
    }

    private void RefreshItems() {
        if (_itemsHost == null) {
            return;
        }
        _itemsHost.View = _view;
        _itemsHost.ItemDefaults = this.CreateDefaultsText();
        _itemsHost.VerticalOffset = this.VerticalOffset;
        _itemsHost.ItemHeight = this.ItemHeight;
        _itemsHost.EnsureViewportMaterialization();
    }

    private TextBlock CreateDefaultsText() {
        TextBlock defaults = new TextBlock();
        defaults.FontSize = this.FontSize;
        defaults.FontFamily = this.FontFamily;
        defaults.FontWeight = this.FontWeight;
        defaults.Foreground = this.Foreground;
        return defaults;
    }

    protected override LayoutSize MeasureOverride(LayoutSize availableSize) {
        if (_itemsHost == null) {
            return new LayoutSize(0.0, 0.0);
        }
        _itemsHost.View = _view;
        _itemsHost.ItemDefaults = this.CreateDefaultsText();
        _itemsHost.VerticalOffset = this.VerticalOffset;
        _itemsHost.ItemHeight = this.ItemHeight;
        _itemsHost.Measure(availableSize);
        return new LayoutSize(_itemsHost.DesiredSize.Width, _itemsHost.ExtentHeight);
    }

    protected override void ArrangeOverride(LayoutSize finalSize) {
        if (_itemsHost != null) {
            _itemsHost.Arrange(finalSize);
        }
    }
}
