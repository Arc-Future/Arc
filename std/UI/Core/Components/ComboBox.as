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
// Popup{ListView}：几何取 chrome 镜像绝对坐标（FrameworkElement.LayoutX/Y 契约：
// 相对窗口根），选项点击经 ListView 选中回调联动 SelectIndex 并关闭；同窗口静态
// 互斥至多一个展开）。v1 下拉底色固定白（主题化另排）；选项超出窗口底部被渲染
// 裁剪（滚动/向上翻转定位另排）；蒙层模态语义（点击外部关闭）由 Popup 承担。

namespace Arc.UI.Components;

using Arc.ComponentModel;
using Arc.UI;
using Arc.UI.Components.Primitives;

/// <summary>
/// ComboBox 非泛型基座（内部）。承载下拉弹层轨与选中显示名（SelectedText DP，
/// 渲染端 chrome 消费面）；选中索引继承基类 SelectedIndex DP（单一状态源），
/// 泛型派生提供强类型数据源与回读。不对开发者暴露。
/// </summary>
internal class ComboBoxBase : Selector {
    /// <summary>展开态弹层（首次 chrome 点击构建，复用至销毁）。</summary>
    protected Popup _dropDown;
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

    // ===== 下拉轨（Popup{ListView}，见文件头诚实边界）=====
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
    /// 下拉联动选中入口（选项行点击回调）：泛型派生覆写为 SelectIndex（校验 +
    /// 选中面同步 + SelectionChanged）；基座默认 no-op。
    /// </summary>
    protected virtual void ApplySelectedIndex(int index) {
    }

    /// <summary>
    /// 展开/刷新下拉：互斥关闭其他实例 → 生存期构建 Popup{ListView} → 按 chrome
    /// 镜像几何定位（chrome 正下方、等宽）→ 呈现属性与选项源重注入（下拉不在本
    /// 控件逻辑树内，FontSize 等环境属性继承断链须显式同步；每次展开重注入当前
    /// 数据源视图（选项行经视图投影物化，SetOptions 换源后所见即所绑）→ Open。
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
        _dropDownList.Background = "#FFFFFFFF";
        _dropDownList.Foreground = this.Foreground;
        _dropDownList.FontSize = this.FontSize;
        _dropDownList.ItemHeight = rowH;
        _dropDownList.Width = dropW;
        _dropDownList.Height = rowH * (double)count;
        _dropDownList.ItemsSource = items;
        _dropDown.PlacementX = chromeX;
        _dropDown.PlacementY = chromeY + chromeH;
        _dropDown.Open();
        if (_dropDown.IsOpen) {
            _activeCombo = this;
        }
    }

    /// <summary>折叠下拉（蒙层点击关闭与 chrome 再点共用；幂等）。</summary>
    void CloseDropDown() {
        if (_dropDown != null && _dropDown.IsOpen) {
            _dropDown.Close();
        }
    }

    /// <summary>生存期一次组装：两回调均静态方法组（无 env 无悬垂，见区块注释）。</summary>
    void BuildDropDown() {
        _dropDown = new Popup();
        _dropDownList = new ListView();
        Action<string> selectedHandler = ComboBoxBase.OnDropDownItemSelectedStatic;
        _dropDownList.OnSelectionChanged(selectedHandler);
        Action<bool> closedHandler = ComboBoxBase.OnDropDownClosedStatic;
        _dropDown.OnClosed(closedHandler);
        _dropDown.Child = _dropDownList;
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
    /// </summary>
    protected override LayoutSize MeasureOverride(LayoutSize availableSize) {
        return this.EstimateRowMetrics();
    }

    /// <summary>折叠态不排布选项宿主（同测量语义：选项列表属展开 Popup 轨）。</summary>
    protected override void ArrangeOverride(LayoutSize finalSize) {
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
        this.SetValue<int>(SelectedIndexProperty, -1);
        this.SetValue<string>(SelectedTextProperty, "");
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

    /// <summary>选中写点：基类 SelectedIndex DP 之外附加 SelectedText 显示名同步。</summary>
    protected override void ApplySelectedIndexCore(int index) {
        base.ApplySelectedIndexCore(index);
        string display = "";
        if (index >= 0) {
            display = _options.Get(index).DisplayName;
        }
        this.SetValue<string>(SelectedTextProperty, display);
    }

    /// <summary>下拉联动选中：转 SelectIndex（校验 + 选中面同步 + SelectionChanged）。</summary>
    protected override void ApplySelectedIndex(int index) {
        this.SelectIndex(index);
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
