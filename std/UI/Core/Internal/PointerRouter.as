// RFC 037 D10.4 + D10.6 · RFC 037 Internal: platform hit -> Arc control callback routing.
//
// platform pointer events report platform handle (RtUiElement* as i64);
// Install before Show maps handle back to Arc control (Button / ToggleButton /
// CheckBox / Slider / ListView / DataGrid / ComboBox)——按 TypeName 的全局回调
// 注册亦集中于此（含 Popup 蒙层 PopupBackdrop），每 Show 会话重注册。
//
// Draft: fixed-slot registry (List/Dictionary 单态化未就绪；≤16 interactive controls/Show).

namespace Arc.UI.Internal;

using Arc.UI.Components;

/// <summary>Platform hit to Arc control router (valid for Show lifetime).</summary>
internal class PointerRouter {
    static int _slotCount = 0;
    static long _handle0 = 0;
    static long _handle1 = 0;
    static long _handle2 = 0;
    static long _handle3 = 0;
    static long _handle4 = 0;
    static long _handle5 = 0;
    static long _handle6 = 0;
    static long _handle7 = 0;
    static long _handle8 = 0;
    static long _handle9 = 0;
    static long _handle10 = 0;
    static long _handle11 = 0;
    static long _handle12 = 0;
    static long _handle13 = 0;
    static long _handle14 = 0;
    static long _handle15 = 0;
    static Button _button0 = null;
    static Button _button1 = null;
    static Button _button2 = null;
    static Button _button3 = null;
    static Button _button4 = null;
    static Button _button5 = null;
    static Button _button6 = null;
    static Button _button7 = null;
    static Button _button8 = null;
    static Button _button9 = null;
    static Button _button10 = null;
    static Button _button11 = null;
    static Button _button12 = null;
    static Button _button13 = null;
    static Button _button14 = null;
    static Button _button15 = null;
    static ToggleButton _toggle0 = null;
    static ToggleButton _toggle1 = null;
    static ToggleButton _toggle2 = null;
    static ToggleButton _toggle3 = null;
    static ToggleButton _toggle4 = null;
    static ToggleButton _toggle5 = null;
    static ToggleButton _toggle6 = null;
    static ToggleButton _toggle7 = null;
    static ToggleButton _toggle8 = null;
    static ToggleButton _toggle9 = null;
    static ToggleButton _toggle10 = null;
    static ToggleButton _toggle11 = null;
    static ToggleButton _toggle12 = null;
    static ToggleButton _toggle13 = null;
    static ToggleButton _toggle14 = null;
    static ToggleButton _toggle15 = null;
    static Slider _slider0 = null;
    static Slider _slider1 = null;
    static Slider _slider2 = null;
    static Slider _slider3 = null;
    static Slider _slider4 = null;
    static Slider _slider5 = null;
    static Slider _slider6 = null;
    static Slider _slider7 = null;
    static Slider _slider8 = null;
    static Slider _slider9 = null;
    static Slider _slider10 = null;
    static Slider _slider11 = null;
    static Slider _slider12 = null;
    static Slider _slider13 = null;
    static Slider _slider14 = null;
    static Slider _slider15 = null;
    static ListView _listView0 = null;
    static ListView _listView1 = null;
    static ListView _listView2 = null;
    static ListView _listView3 = null;
    static ListView _listView4 = null;
    static ListView _listView5 = null;
    static ListView _listView6 = null;
    static ListView _listView7 = null;
    static ListView _listView8 = null;
    static ListView _listView9 = null;
    static ListView _listView10 = null;
    static ListView _listView11 = null;
    static ListView _listView12 = null;
    static ListView _listView13 = null;
    static ListView _listView14 = null;
    static ListView _listView15 = null;
    static DataGrid _dataGrid0 = null;
    static DataGrid _dataGrid1 = null;
    static DataGrid _dataGrid2 = null;
    static DataGrid _dataGrid3 = null;
    static DataGrid _dataGrid4 = null;
    static DataGrid _dataGrid5 = null;
    static DataGrid _dataGrid6 = null;
    static DataGrid _dataGrid7 = null;
    static DataGrid _dataGrid8 = null;
    static DataGrid _dataGrid9 = null;
    static DataGrid _dataGrid10 = null;
    static DataGrid _dataGrid11 = null;
    static DataGrid _dataGrid12 = null;
    static DataGrid _dataGrid13 = null;
    static DataGrid _dataGrid14 = null;
    static DataGrid _dataGrid15 = null;
    static ComboBoxBase _combo0 = null;
    static ComboBoxBase _combo1 = null;
    static ComboBoxBase _combo2 = null;
    static ComboBoxBase _combo3 = null;
    static ComboBoxBase _combo4 = null;
    static ComboBoxBase _combo5 = null;
    static ComboBoxBase _combo6 = null;
    static ComboBoxBase _combo7 = null;
    static ComboBoxBase _combo8 = null;
    static ComboBoxBase _combo9 = null;
    static ComboBoxBase _combo10 = null;
    static ComboBoxBase _combo11 = null;
    static ComboBoxBase _combo12 = null;
    static ComboBoxBase _combo13 = null;
    static ComboBoxBase _combo14 = null;
    static ComboBoxBase _combo15 = null;
    static int _installed = 0;

    private PointerRouter() {
    }

    /// <summary>Clear registration (Window calls before each Show).</summary>
    internal static void Reset() {
        _slotCount = 0;
        _handle0 = 0;
        _handle1 = 0;
        _handle2 = 0;
        _handle3 = 0;
        _handle4 = 0;
        _handle5 = 0;
        _handle6 = 0;
        _handle7 = 0;
        _handle8 = 0;
        _handle9 = 0;
        _handle10 = 0;
        _handle11 = 0;
        _handle12 = 0;
        _handle13 = 0;
        _handle14 = 0;
        _handle15 = 0;
        _button0 = null;
        _button1 = null;
        _button2 = null;
        _button3 = null;
        _button4 = null;
        _button5 = null;
        _button6 = null;
        _button7 = null;
        _button8 = null;
        _button9 = null;
        _button10 = null;
        _button11 = null;
        _button12 = null;
        _button13 = null;
        _button14 = null;
        _button15 = null;
        _toggle0 = null;
        _toggle1 = null;
        _toggle2 = null;
        _toggle3 = null;
        _toggle4 = null;
        _toggle5 = null;
        _toggle6 = null;
        _toggle7 = null;
        _slider0 = null;
        _slider1 = null;
        _slider2 = null;
        _slider3 = null;
        _slider4 = null;
        _slider5 = null;
        _slider6 = null;
        _slider7 = null;
        _listView0 = null;
        _listView1 = null;
        _listView2 = null;
        _listView3 = null;
        _listView4 = null;
        _listView5 = null;
        _listView6 = null;
        _listView7 = null;
        _listView8 = null;
        _listView9 = null;
        _listView10 = null;
        _listView11 = null;
        _listView12 = null;
        _listView13 = null;
        _listView14 = null;
        _listView15 = null;
        _dataGrid0 = null;
        _dataGrid1 = null;
        _dataGrid2 = null;
        _dataGrid3 = null;
        _dataGrid4 = null;
        _dataGrid5 = null;
        _dataGrid6 = null;
        _dataGrid7 = null;
        _dataGrid8 = null;
        _dataGrid9 = null;
        _dataGrid10 = null;
        _dataGrid11 = null;
        _dataGrid12 = null;
        _dataGrid13 = null;
        _dataGrid14 = null;
        _dataGrid15 = null;
        _combo0 = null;
        _combo1 = null;
        _combo2 = null;
        _combo3 = null;
        _combo4 = null;
        _combo5 = null;
        _combo6 = null;
        _combo7 = null;
        _combo8 = null;
        _combo9 = null;
        _combo10 = null;
        _combo11 = null;
        _combo12 = null;
        _combo13 = null;
        _combo14 = null;
        _combo15 = null;
        WindowHost.ClearControlHandlers();
        _installed = 0;
    }

    /// <summary>Register mapping after PlatformTreeSync builds Button node.</summary>
    internal static void RegisterButton(long platformHandle, Button button) {
        if (button == null || platformHandle == 0) {
            return;
        }
        int idx = AllocSlot(platformHandle);
        if (idx < 0) {
            return;
        }
        SetSlotButton(idx, button);
    }

    /// <summary>Register mapping for ToggleButton / CheckBox (toggle-click routing).</summary>
    internal static void RegisterToggle(long platformHandle, ToggleButton toggle) {
        if (toggle == null || platformHandle == 0) {
            return;
        }
        int idx = AllocSlot(platformHandle);
        if (idx < 0) {
            return;
        }
        SetSlotToggle(idx, toggle);
    }

    /// <summary>Register mapping for Slider (drag routing).</summary>
    internal static void RegisterSlider(long platformHandle, Slider slider) {
        if (slider == null || platformHandle == 0) {
            return;
        }
        int idx = AllocSlot(platformHandle);
        if (idx < 0) {
            return;
        }
        SetSlotSlider(idx, slider);
    }

    /// <summary>Register mapping for ListView (selection-click routing).</summary>
    internal static void RegisterListView(long platformHandle, ListView listView) {
        if (listView == null || platformHandle == 0) {
            return;
        }
        int idx = AllocSlot(platformHandle);
        if (idx < 0) {
            return;
        }
        SetSlotListView(idx, listView);
    }

    /// <summary>Register mapping for DataGrid (row-selection-click routing).</summary>
    internal static void RegisterDataGrid(long platformHandle, DataGrid dataGrid) {
        if (dataGrid == null || platformHandle == 0) {
            return;
        }
        int idx = AllocSlot(platformHandle);
        if (idx < 0) {
            return;
        }
        SetSlotDataGrid(idx, dataGrid);
    }

    /// <summary>Register mapping for ComboBox (chrome-click routing).</summary>
    internal static void RegisterComboBox(long platformHandle, ComboBoxBase combo) {
        if (combo == null || platformHandle == 0) {
            return;
        }
        int idx = AllocSlot(platformHandle);
        if (idx < 0) {
            return;
        }
        SetSlotCombo(idx, combo);
    }

    /// <summary>分配共享槽；满则 -1（禁止再写 SetSlot* 覆盖末槽）。</summary>
    static int AllocSlot(long platformHandle) {
        if (platformHandle == 0 || _slotCount >= 16) {
            return -1;
        }
        if (_slotCount == 0) {
            _handle0 = platformHandle;
        } else if (_slotCount == 1) {
            _handle1 = platformHandle;
        } else if (_slotCount == 2) {
            _handle2 = platformHandle;
        } else if (_slotCount == 3) {
            _handle3 = platformHandle;
        } else if (_slotCount == 4) {
            _handle4 = platformHandle;
        } else if (_slotCount == 5) {
            _handle5 = platformHandle;
        } else if (_slotCount == 6) {
            _handle6 = platformHandle;
        } else if (_slotCount == 7) {
            _handle7 = platformHandle;
        } else if (_slotCount == 8) {
            _handle8 = platformHandle;
        } else if (_slotCount == 9) {
            _handle9 = platformHandle;
        } else if (_slotCount == 10) {
            _handle10 = platformHandle;
        } else if (_slotCount == 11) {
            _handle11 = platformHandle;
        } else if (_slotCount == 12) {
            _handle12 = platformHandle;
        } else if (_slotCount == 13) {
            _handle13 = platformHandle;
        } else if (_slotCount == 14) {
            _handle14 = platformHandle;
        } else if (_slotCount == 15) {
            _handle15 = platformHandle;
        }
        int idx = _slotCount;
        _slotCount = _slotCount + 1;
        return idx;
    }

    static void SetSlotButton(int index, Button button) {
        if (button == null) {
            return;
        }
        if (index == 0) { _button0 = button; }
        else if (index == 1) { _button1 = button; }
        else if (index == 2) { _button2 = button; }
        else if (index == 3) { _button3 = button; }
        else if (index == 4) { _button4 = button; }
        else if (index == 5) { _button5 = button; }
        else if (index == 6) { _button6 = button; }
        else if (index == 7) { _button7 = button; }
        else if (index == 8) { _button8 = button; }
        else if (index == 9) { _button9 = button; }
        else if (index == 10) { _button10 = button; }
        else if (index == 11) { _button11 = button; }
        else if (index == 12) { _button12 = button; }
        else if (index == 13) { _button13 = button; }
        else if (index == 14) { _button14 = button; }
        else if (index == 15) { _button15 = button; }
    }

    static void SetSlotToggle(int index, ToggleButton toggle) {
        if (toggle == null) {
            return;
        }
        if (index == 0) { _toggle0 = toggle; }
        else if (index == 1) { _toggle1 = toggle; }
        else if (index == 2) { _toggle2 = toggle; }
        else if (index == 3) { _toggle3 = toggle; }
        else if (index == 4) { _toggle4 = toggle; }
        else if (index == 5) { _toggle5 = toggle; }
        else if (index == 6) { _toggle6 = toggle; }
        else if (index == 7) { _toggle7 = toggle; }
        else if (index == 8) { _toggle8 = toggle; }
        else if (index == 9) { _toggle9 = toggle; }
        else if (index == 10) { _toggle10 = toggle; }
        else if (index == 11) { _toggle11 = toggle; }
        else if (index == 12) { _toggle12 = toggle; }
        else if (index == 13) { _toggle13 = toggle; }
        else if (index == 14) { _toggle14 = toggle; }
        else if (index == 15) { _toggle15 = toggle; }
    }

    static void SetSlotSlider(int index, Slider slider) {
        if (slider == null) {
            return;
        }
        if (index == 0) { _slider0 = slider; }
        else if (index == 1) { _slider1 = slider; }
        else if (index == 2) { _slider2 = slider; }
        else if (index == 3) { _slider3 = slider; }
        else if (index == 4) { _slider4 = slider; }
        else if (index == 5) { _slider5 = slider; }
        else if (index == 6) { _slider6 = slider; }
        else if (index == 7) { _slider7 = slider; }
        else if (index == 8) { _slider8 = slider; }
        else if (index == 9) { _slider9 = slider; }
        else if (index == 10) { _slider10 = slider; }
        else if (index == 11) { _slider11 = slider; }
        else if (index == 12) { _slider12 = slider; }
        else if (index == 13) { _slider13 = slider; }
        else if (index == 14) { _slider14 = slider; }
        else if (index == 15) { _slider15 = slider; }
    }

    static void SetSlotListView(int index, ListView listView) {
        if (listView == null) {
            return;
        }
        if (index == 0) { _listView0 = listView; }
        else if (index == 1) { _listView1 = listView; }
        else if (index == 2) { _listView2 = listView; }
        else if (index == 3) { _listView3 = listView; }
        else if (index == 4) { _listView4 = listView; }
        else if (index == 5) { _listView5 = listView; }
        else if (index == 6) { _listView6 = listView; }
        else if (index == 7) { _listView7 = listView; }
        else if (index == 8) { _listView8 = listView; }
        else if (index == 9) { _listView9 = listView; }
        else if (index == 10) { _listView10 = listView; }
        else if (index == 11) { _listView11 = listView; }
        else if (index == 12) { _listView12 = listView; }
        else if (index == 13) { _listView13 = listView; }
        else if (index == 14) { _listView14 = listView; }
        else if (index == 15) { _listView15 = listView; }
    }

    static void SetSlotDataGrid(int index, DataGrid dataGrid) {
        if (dataGrid == null) {
            return;
        }
        if (index == 0) { _dataGrid0 = dataGrid; }
        else if (index == 1) { _dataGrid1 = dataGrid; }
        else if (index == 2) { _dataGrid2 = dataGrid; }
        else if (index == 3) { _dataGrid3 = dataGrid; }
        else if (index == 4) { _dataGrid4 = dataGrid; }
        else if (index == 5) { _dataGrid5 = dataGrid; }
        else if (index == 6) { _dataGrid6 = dataGrid; }
        else if (index == 7) { _dataGrid7 = dataGrid; }
        else if (index == 8) { _dataGrid8 = dataGrid; }
        else if (index == 9) { _dataGrid9 = dataGrid; }
        else if (index == 10) { _dataGrid10 = dataGrid; }
        else if (index == 11) { _dataGrid11 = dataGrid; }
        else if (index == 12) { _dataGrid12 = dataGrid; }
        else if (index == 13) { _dataGrid13 = dataGrid; }
        else if (index == 14) { _dataGrid14 = dataGrid; }
        else if (index == 15) { _dataGrid15 = dataGrid; }
    }

    static void SetSlotCombo(int index, ComboBoxBase combo) {
        if (combo == null) {
            return;
        }
        if (index == 0) { _combo0 = combo; }
        else if (index == 1) { _combo1 = combo; }
        else if (index == 2) { _combo2 = combo; }
        else if (index == 3) { _combo3 = combo; }
        else if (index == 4) { _combo4 = combo; }
        else if (index == 5) { _combo5 = combo; }
        else if (index == 6) { _combo6 = combo; }
        else if (index == 7) { _combo7 = combo; }
        else if (index == 8) { _combo8 = combo; }
        else if (index == 9) { _combo9 = combo; }
        else if (index == 10) { _combo10 = combo; }
        else if (index == 11) { _combo11 = combo; }
        else if (index == 12) { _combo12 = combo; }
        else if (index == 13) { _combo13 = combo; }
        else if (index == 14) { _combo14 = combo; }
        else if (index == 15) { _combo15 = combo; }
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
        Action<long, int, int> toggleVisual = PointerRouter.RouteControlVisualState;
        WindowHost.SetControlVisualStateHandler("ToggleButton", toggleVisual);
        WindowHost.SetControlVisualStateHandler("CheckBox", toggleVisual);
        Action<long, double> sliderDrag = PointerRouter.RouteSliderDrag;
        WindowHost.SetControlDragHandler("Slider", sliderDrag);
        Action<long> listViewClick = PointerRouter.RouteListViewClick;
        WindowHost.SetControlClickHandler("ListView", listViewClick);
        Action<long> dataGridClick = PointerRouter.RouteDataGridClick;
        WindowHost.SetControlClickHandler("DataGrid", dataGridClick);
        _installed = 1;
    }

    /// <summary>C callback entry: lookup Button by platform handle and RaiseClick.</summary>
    internal static void RouteClick(long platformHandle) {
        Button btn = LookupButton(platformHandle);
        if (btn != null) {
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
        if (toggle != null) {
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
        if (slider != null) {
            slider.ApplyDragValue(value);
        }
    }

    /// <summary>C callback entry (type "ListView"): 命中行 index（C 侧按像素算好写入
    /// 镜像 "HitItemIndex"）→ SelectIndex（SelectedIndex DP + 视觉高亮 + SelectionChanged）。</summary>
    internal static void RouteListViewClick(long platformHandle) {
        ListView listView = LookupListView(platformHandle);
        if (listView != null) {
            double hitIndex = WindowHost.ElementGetNumber(platformHandle, "HitItemIndex", -1.0);
            listView.SelectIndex((int)hitIndex);
        }
    }

    /// <summary>C callback entry (type "DataGrid"): 命中行 index（C 侧按像素算好写入
    /// 镜像 "HitItemIndex"）→ SelectIndex（SelectedIndex DP + 视觉高亮 + SelectionChanged）。</summary>
    internal static void RouteDataGridClick(long platformHandle) {
        DataGrid dataGrid = LookupDataGrid(platformHandle);
        if (dataGrid != null) {
            double hitIndex = WindowHost.ElementGetNumber(platformHandle, "HitItemIndex", -1.0);
            dataGrid.SelectIndex((int)hitIndex);
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
        if (_installed == 0 || platformHandle == 0) {
            return null;
        }
        if (_slotCount > 0 && _handle0 == platformHandle) {
            return _button0;
        }
        if (_slotCount > 1 && _handle1 == platformHandle) {
            return _button1;
        }
        if (_slotCount > 2 && _handle2 == platformHandle) {
            return _button2;
        }
        if (_slotCount > 3 && _handle3 == platformHandle) {
            return _button3;
        }
        if (_slotCount > 4 && _handle4 == platformHandle) {
            return _button4;
        }
        if (_slotCount > 5 && _handle5 == platformHandle) {
            return _button5;
        }
        if (_slotCount > 6 && _handle6 == platformHandle) {
            return _button6;
        }
        if (_slotCount > 7 && _handle7 == platformHandle) {
            return _button7;
        }
        if (_slotCount > 8 && _handle8 == platformHandle) {
            return _button8;
        }
        if (_slotCount > 9 && _handle9 == platformHandle) {
            return _button9;
        }
        if (_slotCount > 10 && _handle10 == platformHandle) {
            return _button10;
        }
        if (_slotCount > 11 && _handle11 == platformHandle) {
            return _button11;
        }
        if (_slotCount > 12 && _handle12 == platformHandle) {
            return _button12;
        }
        if (_slotCount > 13 && _handle13 == platformHandle) {
            return _button13;
        }
        if (_slotCount > 14 && _handle14 == platformHandle) {
            return _button14;
        }
        if (_slotCount > 15 && _handle15 == platformHandle) {
            return _button15;
        }
        return null;
    }

    static ToggleButton LookupToggle(long platformHandle) {
        if (_installed == 0 || platformHandle == 0) {
            return null;
        }
        if (_slotCount > 0 && _handle0 == platformHandle) {
            return _toggle0;
        }
        if (_slotCount > 1 && _handle1 == platformHandle) {
            return _toggle1;
        }
        if (_slotCount > 2 && _handle2 == platformHandle) {
            return _toggle2;
        }
        if (_slotCount > 3 && _handle3 == platformHandle) {
            return _toggle3;
        }
        if (_slotCount > 4 && _handle4 == platformHandle) {
            return _toggle4;
        }
        if (_slotCount > 5 && _handle5 == platformHandle) {
            return _toggle5;
        }
        if (_slotCount > 6 && _handle6 == platformHandle) {
            return _toggle6;
        }
        if (_slotCount > 7 && _handle7 == platformHandle) {
            return _toggle7;
        }
        if (_slotCount > 8 && _handle8 == platformHandle) {
            return _toggle8;
        }
        if (_slotCount > 9 && _handle9 == platformHandle) {
            return _toggle9;
        }
        if (_slotCount > 10 && _handle10 == platformHandle) {
            return _toggle10;
        }
        if (_slotCount > 11 && _handle11 == platformHandle) {
            return _toggle11;
        }
        if (_slotCount > 12 && _handle12 == platformHandle) {
            return _toggle12;
        }
        if (_slotCount > 13 && _handle13 == platformHandle) {
            return _toggle13;
        }
        if (_slotCount > 14 && _handle14 == platformHandle) {
            return _toggle14;
        }
        if (_slotCount > 15 && _handle15 == platformHandle) {
            return _toggle15;
        }
        return null;
    }

    static Slider LookupSlider(long platformHandle) {
        if (_installed == 0 || platformHandle == 0) {
            return null;
        }
        if (_slotCount > 0 && _handle0 == platformHandle) {
            return _slider0;
        }
        if (_slotCount > 1 && _handle1 == platformHandle) {
            return _slider1;
        }
        if (_slotCount > 2 && _handle2 == platformHandle) {
            return _slider2;
        }
        if (_slotCount > 3 && _handle3 == platformHandle) {
            return _slider3;
        }
        if (_slotCount > 4 && _handle4 == platformHandle) {
            return _slider4;
        }
        if (_slotCount > 5 && _handle5 == platformHandle) {
            return _slider5;
        }
        if (_slotCount > 6 && _handle6 == platformHandle) {
            return _slider6;
        }
        if (_slotCount > 7 && _handle7 == platformHandle) {
            return _slider7;
        }
        if (_slotCount > 8 && _handle8 == platformHandle) {
            return _slider8;
        }
        if (_slotCount > 9 && _handle9 == platformHandle) {
            return _slider9;
        }
        if (_slotCount > 10 && _handle10 == platformHandle) {
            return _slider10;
        }
        if (_slotCount > 11 && _handle11 == platformHandle) {
            return _slider11;
        }
        if (_slotCount > 12 && _handle12 == platformHandle) {
            return _slider12;
        }
        if (_slotCount > 13 && _handle13 == platformHandle) {
            return _slider13;
        }
        if (_slotCount > 14 && _handle14 == platformHandle) {
            return _slider14;
        }
        if (_slotCount > 15 && _handle15 == platformHandle) {
            return _slider15;
        }
        return null;
    }

    static ListView LookupListView(long platformHandle) {
        if (_installed == 0 || platformHandle == 0) {
            return null;
        }
        if (_slotCount > 0 && _handle0 == platformHandle) {
            return _listView0;
        }
        if (_slotCount > 1 && _handle1 == platformHandle) {
            return _listView1;
        }
        if (_slotCount > 2 && _handle2 == platformHandle) {
            return _listView2;
        }
        if (_slotCount > 3 && _handle3 == platformHandle) {
            return _listView3;
        }
        if (_slotCount > 4 && _handle4 == platformHandle) {
            return _listView4;
        }
        if (_slotCount > 5 && _handle5 == platformHandle) {
            return _listView5;
        }
        if (_slotCount > 6 && _handle6 == platformHandle) {
            return _listView6;
        }
        if (_slotCount > 7 && _handle7 == platformHandle) {
            return _listView7;
        }
        if (_slotCount > 8 && _handle8 == platformHandle) {
            return _listView8;
        }
        if (_slotCount > 9 && _handle9 == platformHandle) {
            return _listView9;
        }
        if (_slotCount > 10 && _handle10 == platformHandle) {
            return _listView10;
        }
        if (_slotCount > 11 && _handle11 == platformHandle) {
            return _listView11;
        }
        if (_slotCount > 12 && _handle12 == platformHandle) {
            return _listView12;
        }
        if (_slotCount > 13 && _handle13 == platformHandle) {
            return _listView13;
        }
        if (_slotCount > 14 && _handle14 == platformHandle) {
            return _listView14;
        }
        if (_slotCount > 15 && _handle15 == platformHandle) {
            return _listView15;
        }
        return null;
    }

    static DataGrid LookupDataGrid(long platformHandle) {
        if (_installed == 0 || platformHandle == 0) {
            return null;
        }
        if (_slotCount > 0 && _handle0 == platformHandle) {
            return _dataGrid0;
        }
        if (_slotCount > 1 && _handle1 == platformHandle) {
            return _dataGrid1;
        }
        if (_slotCount > 2 && _handle2 == platformHandle) {
            return _dataGrid2;
        }
        if (_slotCount > 3 && _handle3 == platformHandle) {
            return _dataGrid3;
        }
        if (_slotCount > 4 && _handle4 == platformHandle) {
            return _dataGrid4;
        }
        if (_slotCount > 5 && _handle5 == platformHandle) {
            return _dataGrid5;
        }
        if (_slotCount > 6 && _handle6 == platformHandle) {
            return _dataGrid6;
        }
        if (_slotCount > 7 && _handle7 == platformHandle) {
            return _dataGrid7;
        }
        if (_slotCount > 8 && _handle8 == platformHandle) {
            return _dataGrid8;
        }
        if (_slotCount > 9 && _handle9 == platformHandle) {
            return _dataGrid9;
        }
        if (_slotCount > 10 && _handle10 == platformHandle) {
            return _dataGrid10;
        }
        if (_slotCount > 11 && _handle11 == platformHandle) {
            return _dataGrid11;
        }
        if (_slotCount > 12 && _handle12 == platformHandle) {
            return _dataGrid12;
        }
        if (_slotCount > 13 && _handle13 == platformHandle) {
            return _dataGrid13;
        }
        if (_slotCount > 14 && _handle14 == platformHandle) {
            return _dataGrid14;
        }
        if (_slotCount > 15 && _handle15 == platformHandle) {
            return _dataGrid15;
        }
        return null;
    }

    static ComboBoxBase LookupCombo(long platformHandle) {
        if (_installed == 0 || platformHandle == 0) {
            return null;
        }
        if (_slotCount > 0 && _handle0 == platformHandle) {
            return _combo0;
        }
        if (_slotCount > 1 && _handle1 == platformHandle) {
            return _combo1;
        }
        if (_slotCount > 2 && _handle2 == platformHandle) {
            return _combo2;
        }
        if (_slotCount > 3 && _handle3 == platformHandle) {
            return _combo3;
        }
        if (_slotCount > 4 && _handle4 == platformHandle) {
            return _combo4;
        }
        if (_slotCount > 5 && _handle5 == platformHandle) {
            return _combo5;
        }
        if (_slotCount > 6 && _handle6 == platformHandle) {
            return _combo6;
        }
        if (_slotCount > 7 && _handle7 == platformHandle) {
            return _combo7;
        }
        if (_slotCount > 8 && _handle8 == platformHandle) {
            return _combo8;
        }
        if (_slotCount > 9 && _handle9 == platformHandle) {
            return _combo9;
        }
        if (_slotCount > 10 && _handle10 == platformHandle) {
            return _combo10;
        }
        if (_slotCount > 11 && _handle11 == platformHandle) {
            return _combo11;
        }
        if (_slotCount > 12 && _handle12 == platformHandle) {
            return _combo12;
        }
        if (_slotCount > 13 && _handle13 == platformHandle) {
            return _combo13;
        }
        if (_slotCount > 14 && _handle14 == platformHandle) {
            return _combo14;
        }
        if (_slotCount > 15 && _handle15 == platformHandle) {
            return _combo15;
        }
        return null;
    }
}
