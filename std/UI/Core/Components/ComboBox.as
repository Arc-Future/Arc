// Arc.UI.Components — ComboBox<T>：强类型下拉选择（对标 WPF，无运行时反射）。
//
// 业务「枚举 → UI 绑定数据源」的消费端。复用 ItemsControl/ItemContainerGenerator
// 管线物化选项显示名（DisplayName → Text 行），并维护「选中索引 → 枚举值」映射，
// SelectedValue 直接返回强类型 T（非 object 装箱）。
//
// WPF 同构对照：
//   WPF: ComboBox.ItemsSource = 集合；SelectedValue = 当前项值
//   Arc: combo.SetOptions(EnumOptions<T>)（内部经 ItemsSource 物化显示名）；
//        combo.SelectedValue = T
//
// 结构（WPF 同构：Control → ItemsControl → Primitives.Selector → ComboBox）：
//   - `ComboBoxBase`（internal）：非泛型基座，派生 Primitives.Selector——选中索引
//     继承基类 SelectedIndex DP、镜像同步覆写为双写（SelectedIndex + SelectedText），
//     另承载下拉弹层轨；PlatformTreeSync 无需感知具体 T 即可序列化选中态。
//   - `ComboBox<T>`（public）：开发者编码面，绑定 EnumOptions<T>、回读强类型 T；
//     SelectIndex 走基类模板方法（SelectionItemCount/ApplySelectedIndexCore 覆写），
//     通知覆写为强类型 Signal<T>。
//
// 职责边界：
//   - 数据源类型化：SetOptions(EnumOptions<T>) 唯一入口（强类型，杜绝字符串魔法键）
//   - 选中语义：SelectedIndex / SelectedValue / OnSelectionChanged(Action<T>)
//   - 显示物化：复用基类 ItemContainerGenerator（DisplayName → Text）
//
// 诚实边界：下拉「折叠/展开」经 Popup 轨实现（chrome 点击 → RouteChromeClick →
// Popup{ScrollView{ListView}}：几何取 chrome 镜像绝对坐标（FrameworkElement.LayoutX/Y 契约：
// 相对窗口根），选项点击经 ListView 选中回调联动 SelectIndex 并关闭；同窗口静态
// 互斥至多一个展开）。展开定位经 Popup.ComputeInWindowPlacement 窗口内翻（优先
// chrome 正下方，溢出翻上方，两侧不足钳高）；钳高后 ScrollView 外壳承载溢出滚动。
// 下拉底色 / 前景走活动主题 Surface + 环境 Foreground（未设则 Text.Primary）；蒙层轻关闭
// （IsLightDismissEnabled=true：点外部 / Esc）由 Popup 承担；Open(owner) 显式
// 宿主（下拉不在逻辑树，禁仅靠 Parent 上溯）。
// ComboBox&lt;T&gt;：SetOptions 内须全名限定基类 DP（Selector.SelectedIndexProperty /
// ComboBoxBase.SelectedTextProperty），否则 mono 生成伪静态 __static_ComboBox_T_*
// → arc-prune-001。

namespace Arc.UI.Components;

using Arc.ComponentModel;
using Arc.UI;
using Arc.UI.Components.Layout;
using Arc.UI.Components.Primitives;
using Arc.UI.Layout;
using Arc.UI.Styling;

/// <summary>
/// ComboBox 非泛型基座（内部）。承载下拉弹层轨与选中显示名（SelectedText DP，
/// 渲染端 chrome 消费面）；选中索引继承基类 SelectedIndex DP（单一状态源），
/// 泛型派生提供强类型数据源与回读。不对开发者暴露。
/// </summary>
internal class ComboBoxBase : Selector {
    /// <summary>展开态弹层（首次 chrome 点击构建，复用至销毁）。</summary>
    protected Popup _dropDown;
    /// <summary>钳高视口外壳（Popup.Child；滚轮/竖条经 ScrollRouter）。</summary>
    protected ScrollView _dropDownScroll;
    /// <summary>弹层内选项列表（选中联动 + 关闭触发源）。</summary>
    protected ListView _dropDownList;
    /// <summary>当前展开下拉的实例（互斥槽：同窗口至多一个展开；兼作静态回调路由锚点）。</summary>
    protected static ComboBoxBase _activeCombo;

    public static DependencyProperty<string> SelectedTextProperty =
        RegisterProperty<string>(nameof(SelectedText), typeof(ComboBoxBase), "");

    public ComboBoxBase() {
        this.TypeName = "ComboBox";
    }

    /// <summary>
    /// 当前选中项显示名（DP 面）：渲染端 chrome 与平台镜像经此读取，非泛型
    /// 基座因此无需感知 T；由 <see cref="ComboBox{T}.SelectIndex"/> 写入。
    /// </summary>
    public string SelectedText {
        get { return this.GetValue<string>(SelectedTextProperty); }
    }

    /// <summary>SelectedIndex 写入平台镜像（高亮同步）——ComboBox 覆写为双写
    /// （SelectedIndex number + SelectedText string，渲染端 chrome 经此同步）。</summary>
    protected override void SyncMirrorSelection() {
        if (_mirrorHandle != 0) {
            WindowHost.ElementSetNumber(_mirrorHandle, "SelectedIndex", (double)this.SelectedIndex);
            WindowHost.ElementSetString(_mirrorHandle, "SelectedText", this.SelectedText);
        }
    }

    // ===== 下拉轨（Popup{ScrollView{ListView}}，见文件头诚实边界）=====
    //
    // 回调一律静态方法组 + _activeCombo 路由（互斥槽即「当前展开实例」锚点）：
    // 实例方法组 env 经 ByRef 捕获悬垂 → UB（ItemsControl.ObservableCollection
    // 订阅注释同根因），静态无 env 无悬垂。

    /// <summary>PointerRouter chrome 点击入口（RouteComboBoxClick 分发）：切换下拉展开态。</summary>
    internal void RouteChromeClick() {
        if (_dropDown != null && _dropDown.IsOpen) {
            this.CloseDropDown();
            return;
        }
        this.OpenDropDown();
    }

    /// <summary>
    /// 下拉联动选中入口（选项行点击回调）：走 SelectIndex（校验 + SelectedText
    /// 同步 + SelectionChanged）。泛型派生可覆写通知载荷，不必旁路本入口。
    /// </summary>
    protected virtual void ApplySelectedIndex(int index) {
        this.SelectIndex(index);
    }

    /// <summary>选中写点：基类 SelectedIndex + SelectedText（自 View.DisplayAt）。</summary>
    protected override void ApplySelectedIndexCore(int index) {
        base.ApplySelectedIndexCore(index);
        string display = "";
        ItemSourceView view = this.View;
        if (index >= 0 && view != null) {
            display = view.DisplayAt(index);
        }
        this.SetValue<string>(SelectedTextProperty, display);
    }

    /// <summary>
    /// 展开/刷新下拉：互斥关闭其他实例 → 生存期构建 Popup{ScrollView{ListView}} → 按 chrome
    /// 镜像几何 + 窗口内翻定位（chrome 正下方优先、溢出翻上方、两侧不足钳高）→
    /// ScrollView 视口=fittedH、ListView 内容高=preferredH → 呈现属性与选项源重注入 → Open。
    /// </summary>
    void OpenDropDown() {
        ItemSourceView view = this.View;
        int count = 0;
        if (view != null) {
            count = view.Count;
        }
        if (count == 0 || _mirrorHandle == 0) {
            return;
        }
        if (_activeCombo != null && _activeCombo != this) {
            _activeCombo.CloseDropDown();
        }
        if (_dropDown == null) {
            this.BuildDropDown();
        }
        double chromeX = WindowHost.ElementGetNumber(_mirrorHandle, "LayoutX", 0.0);
        double chromeY = WindowHost.ElementGetNumber(_mirrorHandle, "LayoutY", 0.0);
        double chromeW = WindowHost.ElementGetNumber(_mirrorHandle, "LayoutWidth", 0.0);
        double chromeH = WindowHost.ElementGetNumber(_mirrorHandle, "LayoutHeight", 0.0);
        double dropW = chromeW;
        if (dropW <= 0.0) {
            dropW = InputMetrics.MinWidth;
        }
        double rowH = this.EstimateRowMetrics().Height;
        double preferredH = rowH * (double)count;
        double winW = 0.0;
        double winH = 0.0;
        this.ResolveOwnerWindowSize(ref winW, ref winH);
        double placeX = chromeX;
        double placeY = chromeY + chromeH;
        double fittedH = preferredH;
        Popup.ComputeInWindowPlacement(
            chromeX, chromeY + chromeH, chromeY,
            dropW, preferredH, winW, winH,
            ref placeX, ref placeY, ref fittedH);
        // 下拉不在逻辑子树：环境属性不自动继承；显式注入主题 Surface + 前景。
        string surface = "#00000000";
        if (Application.Current != null) {
            string resolved = Application.Current.ResolveColor(BuiltInTheme.Surface);
            if (resolved != null && resolved.Length > 0) {
                surface = resolved;
            }
        }
        _dropDownList.Background = surface;
        if (this.HasAmbientValue(Control.ForegroundProperty.Id)) {
            _dropDownList.Foreground = this.Foreground;
        } else if (Application.Current != null) {
            string textPrimary = Application.Current.ResolveColor(BuiltInTheme.TextPrimary);
            if (textPrimary != null && textPrimary.Length > 0) {
                _dropDownList.Foreground = textPrimary;
            } else {
                _dropDownList.Foreground = this.Foreground;
            }
        } else {
            _dropDownList.Foreground = this.Foreground;
        }
        _dropDownList.FontSize = this.FontSize;
        _dropDownList.ItemHeight = rowH;
        _dropDownList.Width = dropW;
        // 内容取全高；视口钳在 ScrollView（溢出可滚）。
        _dropDownList.Height = preferredH;
        _dropDownList.ItemsSource = view;
        _dropDownScroll.Background = surface;
        _dropDownScroll.Width = dropW;
        _dropDownScroll.Height = fittedH;
        _dropDownScroll.VerticalOffset = 0.0;
        _dropDown.PlacementX = placeX;
        _dropDown.PlacementY = placeY;
        _dropDown.IsLightDismissEnabled = true;
        Window? ownerWin = this.ResolveOwnerWindow();
        _dropDown.Open(ownerWin);
        if (_dropDown.IsOpen) {
            _activeCombo = this;
        }
    }

    /// <summary>
    /// 解析宿主窗口（上溯逻辑树 → Application.MainWindow），供内翻定位与 Popup.Open。
    /// </summary>
    Window? ResolveOwnerWindow() {
        Window? owner = null;
        Element? node = this.Parent;
        while (node != null && owner == null) {
            if (node is Window) {
                owner = (Window)node;
            } else {
                node = node?.Parent;
            }
        }
        if (owner == null && Application.Current != null) {
            owner = Application.Current.MainWindow;
        }
        return owner;
    }

    /// <summary>
    /// 解析宿主窗口客户区尺寸（失败时与 Popup.Open 同款默认值），供内翻定位消费。
    /// </summary>
    void ResolveOwnerWindowSize(ref double winW, ref double winH) {
        Window? owner = this.ResolveOwnerWindow();
        double w = 0.0;
        double h = 0.0;
        if (owner != null) {
            w = owner.Width;
            h = owner.Height;
            if (w <= 0.0 && owner.DesiredSize.Width > 0.0) {
                w = owner.DesiredSize.Width;
            }
            if (h <= 0.0 && owner.DesiredSize.Height > 0.0) {
                h = owner.DesiredSize.Height;
            }
        }
        if (w <= 0.0) {
            w = 720.0;
        }
        if (h <= 0.0) {
            h = 480.0;
        }
        winW = w;
        winH = h;
    }

    /// <summary>折叠下拉（蒙层点击关闭与 chrome 再点共用；幂等）。</summary>
    void CloseDropDown() {
        if (_dropDown != null && _dropDown.IsOpen) {
            _dropDown.Close();
        }
    }

    /// <summary>生存期一次组装：ScrollView 钳高外壳 + ListView；回调静态方法组。</summary>
    void BuildDropDown() {
        _dropDown = new Popup();
        _dropDownScroll = new ScrollView();
        _dropDownScroll.TypeName = "ScrollView";
        _dropDownScroll.VerticalScrollBarVisibility = ScrollBarVisibility.Auto;
        _dropDownScroll.HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled;
        _dropDownList = new ListView();
        Action<string> selectedHandler = ComboBoxBase.OnDropDownItemSelectedStatic;
        _dropDownList.OnSelectionChanged(selectedHandler);
        Action<bool> closedHandler = ComboBoxBase.OnDropDownClosedStatic;
        _dropDown.OnClosed(closedHandler);
        // Children 供 PlatformTreeSync 建树；Content 供 Measure/Arrange 解析。
        _dropDownScroll.Content = _dropDownList;
        _dropDownScroll.AddChild(_dropDownList);
        _dropDown.Child = _dropDownScroll;
    }

    /// <summary>选项行点击静态路由：经互斥槽锚点处理（路由时该实例必为展开者）。</summary>
    static void OnDropDownItemSelectedStatic(string itemText) {
        ComboBoxBase combo = _activeCombo;
        if (combo != null) {
            combo.HandleDropDownItemSelected();
        }
    }

    /// <summary>选项选择：联动选中（同值不重触发变更信号）后关闭下拉。</summary>
    void HandleDropDownItemSelected() {
        int index = _dropDownList.SelectedIndex;
        if (index != this.SelectedIndex) {
            this.ApplySelectedIndex(index);
        }
        this.CloseDropDown();
    }

    /// <summary>蒙层关闭静态路由：确认弹层已关后复位互斥槽。</summary>
    static void OnDropDownClosedStatic(bool isOpen) {
        ComboBoxBase combo = _activeCombo;
        if (combo != null && (combo._dropDown == null || !combo._dropDown.IsOpen)) {
            _activeCombo = null;
        }
    }

    /// <summary>折叠 chrome / 下拉行共用单行度量（文本估算 + 最小值兜底）。</summary>
    LayoutSize EstimateRowMetrics() {
        double fontSize = this.FontSize;
        if (fontSize <= 0.0) {
            fontSize = InputMetrics.FontSizeFallback;
        }
        LayoutSize est = LayoutHelper.EstimateTextSize(
            this.SelectedText, fontSize, InputMetrics.PadX, InputMetrics.PadY,
            this.FontFamily, this.FontWeight);
        double w = est.Width;
        double h = est.Height;
        if (w < InputMetrics.MinWidth) {
            w = InputMetrics.MinWidth;
        }
        if (h < InputMetrics.MinHeight) {
            h = InputMetrics.MinHeight;
        }
        return new LayoutSize(w, h);
    }

    /// <summary>
    /// 折叠态单行测量：选项列表属展开 Popup 轨，不参与主布局测量（基类报告
    /// 全部选项堆叠总高，与折叠 chrome 语义冲突——单行高由文本度量 + 最小值兜底）。
    /// 有默认 Template 时测量 PART_Chrome 并保底 MinWidth/MinHeight。
    /// </summary>
    protected override LayoutSize MeasureOverride(LayoutSize availableSize) {
        if (this.HasTemplateVisual()) {
            LayoutSize templated = this.MeasureTemplateVisual(availableSize);
            LayoutSize row = this.EstimateRowMetrics();
            double tw = templated.Width;
            double th = templated.Height;
            if (tw < row.Width) {
                tw = row.Width;
            }
            if (th < row.Height) {
                th = row.Height;
            }
            if (this.Width > 0.0) {
                tw = this.Width;
            }
            if (this.Height > 0.0) {
                th = this.Height;
            }
            return new LayoutSize(tw, th);
        }
        return this.EstimateRowMetrics();
    }

    /// <summary>折叠态：有模板则排布 PART；无模板不排布选项宿主（Popup 另轨）。</summary>
    protected override void ArrangeOverride(LayoutSize finalSize) {
        if (this.HasTemplateVisual()) {
            this.ArrangeTemplateVisual(finalSize);
        }
    }
}

/// <summary>
/// 强类型下拉选择控件。经 <see cref="SetOptions"/> 绑定 <see cref="EnumOptions{T}"/>，
/// <see cref="SelectedValue"/> 返回选中的枚举值。
/// </summary>
/// <typeparam name="T">枚举类型。</typeparam>
public class ComboBox<T> : ComboBoxBase {
    private EnumOptions<T> _options;

    public ComboBox() {
        this.SelectionChanged = new Signal<T>();
    }

    // ===== 数据源 =====

    /// <summary>
    /// 绑定枚举选项集合（强类型唯一入口）。经强类型视图（本体 = 枚举值 T、投影 =
    /// DisplayName）走基类 <see cref="ItemsSource"/> 唯一数据入口物化（ItemsControl
    /// 单一惯用法，无命令式 Set* 旁路）；选中项本体经视图 ItemAt 直取枚举值。
    /// </summary>
    /// <param name="options">枚举选项集合；null 清空。</param>
    public void SetOptions(EnumOptions<T> options) {
        _options = options;
        this.SetValue<int>(Selector.SelectedIndexProperty, -1);
        this.SetValue<string>(ComboBoxBase.SelectedTextProperty, "");
        if (options == null) {
            this.ItemsSource = null;
            return;
        }
        this.ItemsSource = ItemSourceView.From<T>(options);
    }

    /// <summary>选项总数；未绑定返回 0。</summary>
    public int OptionCount {
        get {
            if (_options == null) {
                return 0;
            }
            return _options.Count;
        }
    }

    // ===== 选择语义 =====

    /// <summary>
    /// 当前选中的枚举值（强类型 T）。仅当 <see cref="SelectedIndex"/> 有效时有效。
    /// </summary>
    public T SelectedValue {
        get { return _options.ValueAt(this.SelectedIndex); }
    }

    /// <summary>可选条目总数：选项数量（SelectIndex 校验上界）。</summary>
    protected override int SelectionItemCount() {
        return this.OptionCount;
    }

    /// <summary>选中写点：基座已同步 SelectedText（View.DisplayAt）；派生无附加写点。</summary>
    protected override void ApplySelectedIndexCore(int index) {
        base.ApplySelectedIndexCore(index);
    }

    /// <summary>下拉联动选中：基座 ApplySelectedIndex → SelectIndex。</summary>
    protected override void ApplySelectedIndex(int index) {
        base.ApplySelectedIndex(index);
    }

    // ===== 选择变更通知（Signal 单引擎，与 ListView.OnSelectionChanged 同惯用法）=====

    /// <summary>选择变更信号——SelectIndex 后触发，载荷为选中枚举值 T。</summary>
    public Signal<T> SelectionChanged;

    /// <summary>订阅选择变更——便捷封装（同 ListView.OnSelectionChanged 惯例）。</summary>
    /// <param name="handler">变更回调（接收新选中枚举值）。</param>
    public void OnSelectionChanged(Action<T> handler) {
        if (SelectionChanged != null && handler != null) {
            SelectionChanged.Subscribe(handler);
        }
    }

    /// <summary>触发选择变更——覆写基类通知步：强类型 Signal&lt;T&gt; 载荷选中枚举值
    /// （同名实例字段与基类 Signal&lt;string&gt; 共享布局槽，本类 ctor 后写覆写真身）。</summary>
    protected override void RaiseSelectionChanged() {
        if (SelectionChanged != null) {
            SelectionChanged.Set(this.SelectedValue);
        }
    }
}
