// RFC 037 D10.4 + D10.6 · RFC 037 Internal: platform hit -> Arc control callback routing.
//
// platform pointer events report platform handle (RtUiElement* as i64);
// Install before Show maps handle back to Arc control (Button / ToggleButton /
// CheckBox / Slider / ListView / DataGrid / ComboBox)——按 TypeName 的全局回调
// 注册亦集中于此（含 Popup 蒙层 PopupBackdrop），每 Show 会话重注册。
// 多弹层：命中逆序优先顶层 PopupBackdrop（Z 序 = 窗口根 children 末尾）；
// RoutePopupBackdropClick → Popup.RouteBackdropClick 按句柄关匹配实例。
//
// 句柄表：`Dictionary<long, T>`（无固定槽上限；ArmlDemo 多页签交互控件可并存）。

namespace Arc.UI.Internal;

using Arc.Collections;
using Arc.UI.Components;

/// <summary>Platform hit to Arc control router (valid for Show lifetime).</summary>
internal class PointerRouter {
    static Dictionary<long, Button> _buttons;
    static Dictionary<long, ToggleButton> _toggles;
    static Dictionary<long, Slider> _sliders;
    static Dictionary<long, ListView> _listViews;
    static Dictionary<long, DataGrid> _dataGrids;
    static Dictionary<long, ComboBoxBase> _combos;
    static Dictionary<long, TabControl> _tabs;
    static Dictionary<long, TreeView> _treeViews;
    static int _installed;

    private PointerRouter() {
    }

    /// <summary>Clear registration (Window calls before each Show).</summary>
    internal static void Reset() {
        if (_buttons != null) {
            _buttons.Clear();
        }
        if (_toggles != null) {
            _toggles.Clear();
        }
        if (_sliders != null) {
            _sliders.Clear();
        }
        if (_listViews != null) {
            _listViews.Clear();
        }
        if (_dataGrids != null) {
            _dataGrids.Clear();
        }
        if (_combos != null) {
            _combos.Clear();
        }
        if (_tabs != null) {
            _tabs.Clear();
        }
        if (_treeViews != null) {
            _treeViews.Clear();
        }
        WindowHost.ClearControlHandlers();
        _installed = 0;
    }

    /// <summary>Register mapping after PlatformTreeSync builds Button node.</summary>
    internal static void RegisterButton(long platformHandle, Button button) {
        if (button == null || platformHandle == 0) {
            return;
        }
        if (_buttons == null) {
            _buttons = new Dictionary<long, Button>();
        }
        _buttons[platformHandle] = button;
    }

    /// <summary>Register mapping for ToggleButton / CheckBox (toggle-click routing).</summary>
    internal static void RegisterToggle(long platformHandle, ToggleButton toggle) {
        if (toggle == null || platformHandle == 0) {
            return;
        }
        if (_toggles == null) {
            _toggles = new Dictionary<long, ToggleButton>();
        }
        _toggles[platformHandle] = toggle;
    }

    /// <summary>Register mapping for Slider (drag routing).</summary>
    internal static void RegisterSlider(long platformHandle, Slider slider) {
        if (slider == null || platformHandle == 0) {
            return;
        }
        if (_sliders == null) {
            _sliders = new Dictionary<long, Slider>();
        }
        _sliders[platformHandle] = slider;
    }

    /// <summary>Register mapping for ListView (selection-click routing).</summary>
    internal static void RegisterListView(long platformHandle, ListView listView) {
        if (listView == null || platformHandle == 0) {
            return;
        }
        if (_listViews == null) {
            _listViews = new Dictionary<long, ListView>();
        }
        _listViews[platformHandle] = listView;
    }

    /// <summary>Register mapping for DataGrid (row-selection-click routing).</summary>
    internal static void RegisterDataGrid(long platformHandle, DataGrid dataGrid) {
        if (dataGrid == null || platformHandle == 0) {
            return;
        }
        if (_dataGrids == null) {
            _dataGrids = new Dictionary<long, DataGrid>();
        }
        _dataGrids[platformHandle] = dataGrid;
    }

    /// <summary>Register mapping for ComboBox (chrome-click routing).</summary>
    internal static void RegisterComboBox(long platformHandle, ComboBoxBase combo) {
        if (combo == null || platformHandle == 0) {
            return;
        }
        if (_combos == null) {
            _combos = new Dictionary<long, ComboBoxBase>();
        }
        _combos[platformHandle] = combo;
    }

    /// <summary>Register mapping for TabControl (tab-bar click routing).</summary>
    internal static void RegisterTabControl(long platformHandle, TabControl tabs) {
        if (tabs == null || platformHandle == 0) {
            return;
        }
        if (_tabs == null) {
            _tabs = new Dictionary<long, TabControl>();
        }
        _tabs[platformHandle] = tabs;
    }

    /// <summary>ScrollRouter 顶栏滚轮：按平台句柄取 TabControl。</summary>
    internal static TabControl FindTabControl(long platformHandle) {
        return LookupTabControl(platformHandle);
    }

    /// <summary>Register mapping for TreeView (row/expander click routing).</summary>
    internal static void RegisterTreeView(long platformHandle, TreeView tree) {
        if (tree == null || platformHandle == 0) {
            return;
        }
        if (_treeViews == null) {
            _treeViews = new Dictionary<long, TreeView>();
        }
        _treeViews[platformHandle] = tree;
    }

    /// <summary>Install C->Arc callbacks (Window.Show before message loop).</summary>
    internal static void Install() {
        Action<long> clickHandler = PointerRouter.RouteClick;
        WindowHost.SetButtonClickHandler(clickHandler);
        Action<long, int, int> visualHandler = PointerRouter.RouteVisualState;
        WindowHost.SetButtonVisualStateHandler(visualHandler);
        Action<long> toggleClick = PointerRouter.RouteControlClick;
        WindowHost.SetControlClickHandler("ToggleButton", toggleClick);
        WindowHost.SetControlClickHandler("CheckBox", toggleClick);
        WindowHost.SetControlClickHandler("RadioButton", toggleClick);
        Action<long, int, int> toggleVisual = PointerRouter.RouteControlVisualState;
        WindowHost.SetControlVisualStateHandler("ToggleButton", toggleVisual);
        WindowHost.SetControlVisualStateHandler("CheckBox", toggleVisual);
        WindowHost.SetControlVisualStateHandler("RadioButton", toggleVisual);
        Action<long, double> sliderDrag = PointerRouter.RouteSliderDrag;
        WindowHost.SetControlDragHandler("Slider", sliderDrag);
        Action<long, int, int> sliderVisual = PointerRouter.RouteSliderVisualState;
        WindowHost.SetControlVisualStateHandler("Slider", sliderVisual);
        // TextBox 选区拖拽：C 侧载荷为局部 DIP X（见 rt_ui_dispatch_control_drag）。
        Action<long, double> textBoxDrag = ImeBridge.RouteInputDrag;
        WindowHost.SetControlDragHandler("TextBox", textBoxDrag);
        WindowHost.SetControlDragHandler("PasswordBox", textBoxDrag);
        Action<long> listViewClick = PointerRouter.RouteListViewClick;
        WindowHost.SetControlClickHandler("ListView", listViewClick);
        Action<long> dataGridClick = PointerRouter.RouteDataGridClick;
        WindowHost.SetControlClickHandler("DataGrid", dataGridClick);
        Action<long> tabClick = PointerRouter.RouteTabControlClick;
        WindowHost.SetControlClickHandler("TabControl", tabClick);
        Action<long, int, int> tabVisual = PointerRouter.RouteTabControlVisualState;
        WindowHost.SetControlVisualStateHandler("TabControl", tabVisual);
        Action<long> treeClick = PointerRouter.RouteTreeViewClick;
        WindowHost.SetControlClickHandler("TreeView", treeClick);
        Action<long> comboClick = PointerRouter.RouteComboBoxClick;
        WindowHost.SetControlClickHandler("ComboBox", comboClick);
        Action<long> backdropClick = PointerRouter.RoutePopupBackdropClick;
        WindowHost.SetControlClickHandler("PopupBackdrop", backdropClick);
        _installed = 1;
    }

    /// <summary>C callback（type "PopupBackdrop"）：蒙层点击 → Popup 轻关闭路由。</summary>
    internal static void RoutePopupBackdropClick(long platformHandle) {
        Popup.RouteBackdropClick(platformHandle);
    }

    /// <summary>C callback entry: lookup Button by platform handle and RaiseClick.</summary>
    internal static void RouteClick(long platformHandle) {
        Button btn = LookupButton(platformHandle);
        if (btn != null && btn.IsEnabled) {
            btn.RaiseClick();
        }
    }

    /// <summary>C callback entry: sync IsMouseOver / IsPressed on Arc Button + 平台镜像 + 重绘。</summary>
    internal static void RouteVisualState(long platformHandle, int isMouseOver, int isPressed) {
        Button btn = LookupButton(platformHandle);
        // 状态写平台镜像（渲染器读镜像消费状态色）+ 触发按需重绘（A-1②）。
        WindowHost.ElementSetBool(platformHandle, "IsMouseOver", isMouseOver != 0 ? 1 : 0);
        WindowHost.ElementSetBool(platformHandle, "IsPressed", isPressed != 0 ? 1 : 0);
        FramePump.Invalidate();
        if (btn != null) {
            btn.ApplyPointerState(isMouseOver != 0, isPressed != 0);
        }
    }

    /// <summary>C callback entry (type "ToggleButton"/"CheckBox"): toggle IsChecked on release.</summary>
    internal static void RouteControlClick(long platformHandle) {
        ToggleButton toggle = LookupToggle(platformHandle);
        FramePump.Invalidate();
        if (toggle != null && toggle.IsEnabled) {
            toggle.RaiseToggle();
        }
    }

    /// <summary>C callback entry (type "ToggleButton"/"CheckBox"): sync hover/pressed visual state.</summary>
    internal static void RouteControlVisualState(long platformHandle, int isMouseOver, int isPressed) {
        ToggleButton toggle = LookupToggle(platformHandle);
        WindowHost.ElementSetBool(platformHandle, "IsMouseOver", isMouseOver != 0 ? 1 : 0);
        WindowHost.ElementSetBool(platformHandle, "IsPressed", isPressed != 0 ? 1 : 0);
        FramePump.Invalidate();
        if (toggle != null) {
            toggle.ApplyPointerState(isMouseOver != 0, isPressed != 0);
        }
    }

    /// <summary>C callback entry (type "Slider"): platform-computed value applied to Arc Slider.</summary>
    internal static void RouteSliderDrag(long platformHandle, double value) {
        Slider slider = LookupSlider(platformHandle);
        WindowHost.ElementSetBool(platformHandle, "IsPressed", 1);
        FramePump.Invalidate();
        if (slider != null && slider.IsEnabled) {
            slider.ApplyDragValue(value);
        }
    }

    /// <summary>C callback entry (type "Slider"): sync hover/pressed for track/thumb 态色。</summary>
    internal static void RouteSliderVisualState(long platformHandle, int isMouseOver, int isPressed) {
        WindowHost.ElementSetBool(platformHandle, "IsMouseOver", isMouseOver != 0 ? 1 : 0);
        WindowHost.ElementSetBool(platformHandle, "IsPressed", isPressed != 0 ? 1 : 0);
        FramePump.Invalidate();
    }

    /// <summary>C callback entry (type "ListView"): 命中行 index（C 侧按像素算好写入
    /// 镜像 "HitItemIndex"）→ 焦点停靠 + SelectIndex（SelectedIndex DP + 视觉高亮 + SelectionChanged）。</summary>
    internal static void RouteListViewClick(long platformHandle) {
        ListView listView = LookupListView(platformHandle);
        if (listView != null) {
            FocusManager.FocusPlatformHandle(platformHandle);
            double hitIndex = WindowHost.ElementGetNumber(platformHandle, "HitItemIndex", -1.0);
            listView.SelectIndex((int)hitIndex);
        }
    }

    /// <summary>C callback entry (type "DataGrid"): 命中行 index（镜像 HitItemIndex）+
    /// 修饰键（镜像 HitMods，bit0=Shift bit1=Ctrl）→ SelectIndexWithMods（多选手势 /
    /// 无修饰替换单行；SelectionChanged 直挂）。</summary>
    internal static void RouteDataGridClick(long platformHandle) {
        DataGrid dataGrid = LookupDataGrid(platformHandle);
        if (dataGrid != null) {
            double hitIndex = WindowHost.ElementGetNumber(platformHandle, "HitItemIndex", -1.0);
            int mods = (int)WindowHost.ElementGetNumber(platformHandle, "HitMods", 0.0);
            dataGrid.SelectIndexWithMods((int)hitIndex, mods);
        }
    }

    /// <summary>C callback entry (type "TabControl"): HitTabOverflow 步进 / HitTabClose 关闭 / HitTabIndex → SelectedIndex。</summary>
    internal static void RouteTabControlClick(long platformHandle) {
        TabControl tabs = LookupTabControl(platformHandle);
        if (tabs != null) {
            tabs.SelectHitTab();
        }
    }

    /// <summary>C visual：离开 TabControl 清 HoverTabIndex（页内移动由 C update_hover 写镜像）。</summary>
    internal static void RouteTabControlVisualState(long platformHandle, int isMouseOver, int isPressed) {
        WindowHost.ElementSetBool(platformHandle, "IsMouseOver", isMouseOver != 0 ? 1 : 0);
        WindowHost.ElementSetBool(platformHandle, "IsPressed", isPressed != 0 ? 1 : 0);
        if (isMouseOver == 0) {
            WindowHost.ElementSetNumber(platformHandle, "HoverTabIndex", -1.0);
            TabControl tabs = LookupTabControl(platformHandle);
            if (tabs != null) {
                tabs.ApplyHeaderHover(-1);
            }
        }
        FramePump.Invalidate();
    }

    /// <summary>C callback entry (type "TreeView"): 点击聚焦 + HitItemIndex/HitExpand → RouteHit。</summary>
    internal static void RouteTreeViewClick(long platformHandle) {
        TreeView tree = LookupTreeView(platformHandle);
        if (tree != null) {
            FocusManager.FocusPlatformHandle(platformHandle);
            tree.RouteHit();
        }
    }

    /// <summary>C callback entry (type "ComboBox"): chrome 点击切换展开态
    /// （展开内容为 Popup 轨，见 ComboBoxBase.ToggleDropDown）。</summary>
    internal static void RouteComboBoxClick(long platformHandle) {
        ComboBoxBase combo = LookupCombo(platformHandle);
        if (combo != null) {
            combo.RouteChromeClick();
        }
    }

    static Button LookupButton(long platformHandle) {
        if (_installed == 0 || platformHandle == 0 || _buttons == null) {
            return null;
        }
        Button btn = null;
        if (_buttons.TryGetValue(platformHandle, out btn)) {
            return btn;
        }
        return null;
    }

    static ToggleButton LookupToggle(long platformHandle) {
        if (_installed == 0 || platformHandle == 0 || _toggles == null) {
            return null;
        }
        ToggleButton toggle = null;
        if (_toggles.TryGetValue(platformHandle, out toggle)) {
            return toggle;
        }
        return null;
    }

    static Slider LookupSlider(long platformHandle) {
        if (_installed == 0 || platformHandle == 0 || _sliders == null) {
            return null;
        }
        Slider slider = null;
        if (_sliders.TryGetValue(platformHandle, out slider)) {
            return slider;
        }
        return null;
    }

    static ListView LookupListView(long platformHandle) {
        if (_installed == 0 || platformHandle == 0 || _listViews == null) {
            return null;
        }
        ListView listView = null;
        if (_listViews.TryGetValue(platformHandle, out listView)) {
            return listView;
        }
        return null;
    }

    static DataGrid LookupDataGrid(long platformHandle) {
        if (_installed == 0 || platformHandle == 0 || _dataGrids == null) {
            return null;
        }
        DataGrid dataGrid = null;
        if (_dataGrids.TryGetValue(platformHandle, out dataGrid)) {
            return dataGrid;
        }
        return null;
    }

    static ComboBoxBase LookupCombo(long platformHandle) {
        if (_installed == 0 || platformHandle == 0 || _combos == null) {
            return null;
        }
        ComboBoxBase combo = null;
        if (_combos.TryGetValue(platformHandle, out combo)) {
            return combo;
        }
        return null;
    }

    static TabControl LookupTabControl(long platformHandle) {
        if (_installed == 0 || platformHandle == 0 || _tabs == null) {
            return null;
        }
        TabControl tabs = null;
        if (_tabs.TryGetValue(platformHandle, out tabs)) {
            return tabs;
        }
        return null;
    }

    static TreeView LookupTreeView(long platformHandle) {
        if (_installed == 0 || platformHandle == 0 || _treeViews == null) {
            return null;
        }
        TreeView tree = null;
        if (_treeViews.TryGetValue(platformHandle, out tree)) {
            return tree;
        }
        return null;
    }
}
