// RFC 037 D2.1 / RFC 037 D1: Arc.UI.Components.Primitives — Selector 单选语义层。
//
// WPF 同构层级对照（WPF Selector 对标，Controls.Primitives 同构落位）：
//   WPF: Control → ItemsControl → Primitives.Selector → ListBox → ListView
//   Arc:  Control → ItemsControl → Primitives.Selector → ListView / ComboBoxBase
//         Control → ItemsControl → Primitives.Selector → Primitives.MultiSelector → DataGrid
//
// 职责：单选语义通用封装——SelectedIndex/SelectedItem/SelectedValue/SelectedValuePath
// 四 DP + 平台镜像高亮同步（_mirrorHandle）+ SelectionChanged Signal 通道（RFC 037
// §5.3）。三份同构选择实现（ListView/ComboBoxBase/DataGrid）收敛于此，派生控件只覆写
// 差异点。多选语义（SelectionMode/SelectedItems/SelectItem）由派生层 MultiSelector 承载。
//
// 命名空间（WPF Controls.Primitives 同构理由）：Selector 在 WPF 中即位于
// Controls.Primitives（System.Windows.Controls.Primitives.Selector），本层直接同构落位。
// 常用控件（ListView/ComboBox/DataGrid）仍留 Arc.UI.Components 顶层，开发者日常
// 无需感知 Primitives。
//
// 模板方法模式（无属性变更回调机制的联动方案）：SelectIndex 是唯一选中流程入口
// （校验 → 写点 → 镜像同步 → 附加同步 → 通知），差异点经 protected virtual 钩子插拔：
//   - SelectionItemCount：可选条目总数（校验上界；默认 ItemContainerGenerator.ItemCount）
//   - ApplySelectedIndexCore：选中写点（默认 SelectedIndex DP；ComboBox<T> 附加 SelectedText）
//   - OnSelectionApplied：选中后附加同步（ListView 经视图本体化 SelectedItem；
//     ComboBoxBase 推 SelectedText 镜像串；BindPlatformMirror 复位时亦调用，绑定即同步）
//   - SelectionPayload：string 载荷提取（默认选中项显示投影；DataGrid 行首列文本）
//   - RaiseSelectionChanged：通知触发（默认 Signal<string>；ComboBox<T> 覆写为 Signal<T>）

namespace Arc.UI.Components.Primitives;

using Arc.UI.Components;

/// <summary>承载单选语义的 ItemsControl 派生层（WPF Selector 对标）。</summary>
public class Selector : ItemsControl {
    /// <summary>平台 RtUiElement 句柄；PlatformTreeSync.BindPlatformMirror 写入（SelectedIndex 高亮同步）。</summary>
    protected long _mirrorHandle;

    public Selector() {
        this.SetupSelector();
    }

    /// <summary>自管视口派生（DataGrid）入口：ownsItemsHost=false 跳过基类项宿主装配。</summary>
    protected Selector(bool ownsItemsHost) : base(ownsItemsHost) {
        this.SetupSelector();
    }

    private void SetupSelector() {
        this.Type = typeof(Selector);
        this.TypeName = "Selector";
        this.SelectionChanged = new Signal<string>("");
    }

    // ===== 静态依赖属性元数据（RFC 037 D1 WPF 同构）=====

    /// <summary>SelectedIndex 属性元数据——当前选中项索引，默认 -1（无选中）。</summary>
    public static DependencyProperty<int> SelectedIndexProperty =
        RegisterProperty<int>(nameof(SelectedIndex), typeof(Selector), -1);

    /// <summary>SelectedItem 属性元数据——当前选中项数据，默认 null。</summary>
    public static DependencyProperty<object> SelectedItemProperty =
        RegisterProperty<object>(nameof(SelectedItem), typeof(Selector), null);

    /// <summary>SelectedValue 属性元数据——当前选中项的值（由 SelectedValuePath 提取），默认 null。</summary>
    public static DependencyProperty<object> SelectedValueProperty =
        RegisterProperty<object>(nameof(SelectedValue), typeof(Selector), null);

    /// <summary>SelectedValuePath 属性元数据——选中值提取路径，默认空串。</summary>
    public static DependencyProperty<string> SelectedValuePathProperty =
        RegisterProperty<string>(nameof(SelectedValuePath), typeof(Selector), "");

    // ===== 公共属性 wrapper：委托 Element.GetValue<T>/SetValue<T> =====

    /// <summary>当前选中项索引（-1 表示无选中）。</summary>
    public int SelectedIndex {
        get { return this.GetValue<int>(SelectedIndexProperty); }
        set { this.SetValue<int>(SelectedIndexProperty, value); }
    }

    /// <summary>当前选中项数据。</summary>
    public object SelectedItem {
        get { return this.GetValue<object>(SelectedItemProperty); }
        set { this.SetValue<object>(SelectedItemProperty, value); }
    }

    /// <summary>当前选中项的值（由 SelectedValuePath 从 SelectedItem 提取）。</summary>
    public object SelectedValue {
        get { return this.GetValue<object>(SelectedValueProperty); }
        set { this.SetValue<object>(SelectedValueProperty, value); }
    }

    /// <summary>选中值提取路径（如 "Id" 表示 SelectedValue = SelectedItem.Id）。</summary>
    public string SelectedValuePath {
        get { return this.GetValue<string>(SelectedValuePathProperty); }
        set { this.SetValue<string>(SelectedValuePathProperty, value); }
    }

    // ===== 选择交互（RFC 037 D10.6 · PointerRouter 分发入口）=====

    /// <summary>PointerRouter 点击入口：选中指定项。index ∈ [-1, SelectionItemCount)；
    /// 越界正值忽略（保持现状），-1 取消选择。流程：校验 → 写点 → 平台镜像 →
    /// 附加同步 → SelectionChanged。</summary>
    public void SelectIndex(int index) {
        int count = this.SelectionItemCount();
        if (index < -1 || index >= count) {
            return;
        }
        this.ApplySelectedIndexCore(index);
        this.SyncMirrorSelection();
        this.OnSelectionApplied();
        this.RaiseSelectionChanged();
    }

    /// <summary>可选条目总数（SelectIndex 校验上界）。默认项宿主 ItemCount；
    /// DataGrid 覆写为行数。</summary>
    protected virtual int SelectionItemCount() {
        if (this.ItemContainerGenerator == null) {
            return 0;
        }
        return this.ItemContainerGenerator.ItemCount;
    }

    /// <summary>选中写点。默认写 SelectedIndex DP；ComboBox&lt;T&gt; 附加 SelectedText 同步。</summary>
    protected virtual void ApplySelectedIndexCore(int index) {
        this.SetValue<int>(SelectedIndexProperty, index);
    }

    /// <summary>选中后附加同步。ListView 装箱 SelectedItem；ComboBoxBase 推
    /// SelectedText 镜像串；BindPlatformMirror 复位时亦调用（绑定即同步）。</summary>
    protected virtual void OnSelectionApplied() {
    }

    /// <summary>SelectionChanged 载荷提取。默认选中项文本；DataGrid 覆写为行首列文本。</summary>
    protected virtual string SelectionPayload() {
        return this.SelectedItemText();
    }

    /// <summary>触发选择变更。默认 Signal&lt;string&gt;；ComboBox&lt;T&gt; 覆写为 Signal&lt;T&gt;。</summary>
    protected virtual void RaiseSelectionChanged() {
        if (SelectionChanged != null) {
            SelectionChanged.Set(this.SelectionPayload());
        }
    }

    // ===== 平台镜像高亮同步（PlatformTreeSync 对接）=====

    /// <summary>PlatformTreeSync 调用：登记 mirror（SelectedIndex 高亮同步），绑定即同步。</summary>
    public virtual void BindPlatformMirror(long handle) {
        _mirrorHandle = handle;
        this.SyncMirrorSelection();
        this.OnSelectionApplied();
    }

    /// <summary>SelectedIndex 写入平台镜像（高亮同步）；ComboBoxBase 覆写为双写
    /// （SelectedIndex number + SelectedText string）。</summary>
    protected virtual void SyncMirrorSelection() {
        if (_mirrorHandle != 0) {
            WindowHost.ElementSetNumber(_mirrorHandle, "SelectedIndex",
                (double)this.SelectedIndex);
        }
    }

    /// <summary>读取当前选中项显示投影（经数据源视图 DisplayAt，不受虚拟化
    /// 物化窗口限制；无视图或索引无效返回空串）。</summary>
    protected string SelectedItemText() {
        ItemSourceView view = this.View;
        if (view == null) {
            return "";
        }
        int index = this.SelectedIndex;
        if (index < 0 || index >= view.Count) {
            return "";
        }
        return view.DisplayAt(index);
    }

    /// <summary>按选中项文本装箱 SelectedItem（未选中/未物化为 null）。</summary>
    protected void SyncSelectedItem() {
        string text = this.SelectedItemText();
        object item = null;
        if (text != "") {
            item = text;
        }
        this.SetValue<object>(SelectedItemProperty, item);
    }

    // ===== 控件事件通道（RFC 037 §5.3 · Signal 单引擎 · 与 Button.Clicked/OnClick 同一惯用法）=====
    //
    // SelectionChanged 是选择变更通知：SelectIndex（PointerRouter 点击分发入口）内
    // 写点后统一触发，载荷为**新选中项文本**（index 无效时为空串；DataGrid 覆写为
    // 行首列文本，ComboBox&lt;T&gt; 覆写为强类型 Signal&lt;T&gt;——同名实例字段与基类共享
    // 布局槽，派生 ctor 后写覆写真身）。Signal.Set 无相等性短路，同值赋值仍触发。
    // On* 便捷订阅内部弃 token（常驻订阅随元素销毁确定退订）。
    //
    // 命名决策：既有 `public string SelectionChanged;` 占位字段（ARML typeck 未注册、
    // 全仓零引用的死占位）更名为 SelectionChangedHandler 释放 SelectionChanged 给
    // Signal（同 Slider.ValueChangedHandler 前例；Button Click/Clicked 同构）。

    /// <summary>
    /// 选择变更信号——选中项更新后触发，载荷为新选中项文本。
    /// 订阅示例：
    /// <code>
    ///   lv.OnSelectionChanged(item => DoSomething(item));      // 便捷订阅
    ///   int t = lv.SelectionChanged.Subscribe(x => ...);       // 完整 Subscribe API + token 退订
    /// </code>
    /// </summary>
    public Signal<string> SelectionChanged;

    /// <summary>订阅选择变更——SelectionChanged.Subscribe 的便捷封装（同 Button.OnClick 惯例）。</summary>
    /// <param name="handler">变更回调（接收新选中项文本；-1/未物化行为 ""）。</param>
    public void OnSelectionChanged(Action<string> handler) {
        if (SelectionChanged != null && handler != null) {
            SelectionChanged.Subscribe(handler);
        }
    }

    // ===== 事件路由（RFC 037 §7 不在范围，保持 string 方法名）=====
    //
    // SelectionChangedHandler 是选择变更事件处理器名（指向 .arml.as partial class
    // 中的方法）。事件路由系统由后续独立 RFC 处理；Signal 通道取 SelectionChanged 名。

    /// <summary>选择变更事件处理器名（.arml.as partial class 中的方法名）。</summary>
    public string SelectionChangedHandler;
}
