// RFC 037 M3.6 · RFC 037 Internal: Arc 逻辑树 → 平台 RtUiElement 树一次性同步。
//
// 按 TypeName 分派属性；Content 文本经 ContentHelper.TextOrEmpty 从 Content DP 读取。

namespace Arc.UI.Internal;

using Arc.UI;
using Arc.UI.Components;
using Arc.UI.Components.Layout;
using Arc.UI.Layout;
using Arc.UI.Media;
using Arc.UI.Rendering;
using Arc.UI.Styling;

internal class PlatformTreeSync {
    /// <summary>
    /// 窗口主树代际计数：Window 重建平台主树时自增。弹层等附加层据
    /// 「builtEpoch != RootEpoch」判定旧镜像句柄悬空（跨会话句柄号可被新树
    /// 回收复用）并重走建树路径，见 Popup 文件头诚实边界。
    /// </summary>
    internal static int RootEpoch = 0;

    private PlatformTreeSync() {
    }

    /// <summary>
    /// 同步非继承画刷：仅本地/样式写入才上镜；未设写空串，供渲染回落主题 / VSM。
    /// </summary>
    private static void SyncOwnBrush(long handle, string propName, Element element,
                                       DependencyProperty<Brush> prop) {
        if (element.HasOwnValue(prop.Id)) {
            WindowHost.ElementSetString(handle, propName, element.GetValue<Brush>(prop).ToHex());
        } else {
            WindowHost.ElementSetString(handle, propName, "");
        }
    }

    /// <summary>
    /// 同步环境前景：本地/样式/继承有效值上镜；纯 DP 默认不上镜（空串），
    /// 避免 Light 默认 hex 挡住活动主题 Text.Primary / VSM TextOnAccent。
    /// </summary>
    private static void SyncAmbientForeground(long handle, Element element) {
        if (element.HasAmbientValue(Control.ForegroundProperty.Id)) {
            WindowHost.ElementSetString(handle, "Foreground",
                element.GetValue<Brush>(Control.ForegroundProperty).ToHex());
        } else {
            WindowHost.ElementSetString(handle, "Foreground", "");
        }
    }

    /// <summary>从 Arc 逻辑树根递归构建平台 RtUiElement 树并返回根句柄。</summary>
    internal static long BuildFromArc(Element arcRoot) {
        return PlatformTreeSync.BuildFromArcCore(arcRoot, 0);
    }

    /// <summary>
    /// chromeHostHandle：模板宿主控件句柄。套用 ControlTemplate 的 Control 把自身
    /// 句柄传给模板子树，PART_* 经 ChromeHostHandle/ChromeRole 镜像供 RenderTree
    /// 把 VSM 画到部件（命中仍落宿主——Border/TextBlock 非 pointer target）。
    /// </summary>
    static long BuildFromArcCore(Element arcRoot, long chromeHostHandle) {
        string typeName = arcRoot.TypeName;
        if (typeName == null || typeName == "") {
            typeName = "Element";
        }
        long handle = WindowHost.ElementCreate(typeName);

        if (typeName == "Window") {
            Window window = (Window)arcRoot;
            SyncOwnBrush(handle, "Background", window, Control.BackgroundProperty);
        } else if (typeName == "StackPanel") {
            StackPanel panel = (StackPanel)arcRoot;
            WindowHost.ElementSetString(handle, "Orientation",
                UIEnumConverter.OrientationText(panel.Orientation));
            WindowHost.ElementSetNumber(handle, "Spacing", panel.Spacing);
            SyncOwnBrush(handle, "Background", panel, Panel.BackgroundProperty);
        } else if (typeName == "Grid") {
            Grid grid = (Grid)arcRoot;
            WindowHost.ElementSetNumber(handle, "ColumnSpacing", grid.ColumnSpacing);
            WindowHost.ElementSetNumber(handle, "RowSpacing", grid.RowSpacing);
            SyncOwnBrush(handle, "Background", grid, Panel.BackgroundProperty);
        } else if (typeName == "Canvas") {
            Canvas canvas = (Canvas)arcRoot;
            SyncOwnBrush(handle, "Background", canvas, Panel.BackgroundProperty);
        } else if (typeName == "DockPanel") {
            DockPanel dock = (DockPanel)arcRoot;
            int lastFill = dock.LastChildFill ? 1 : 0;
            WindowHost.ElementSetBool(handle, "LastChildFill", lastFill);
            SyncOwnBrush(handle, "Background", dock, Panel.BackgroundProperty);
        } else if (typeName == "WrapPanel") {
            WrapPanel wrap = (WrapPanel)arcRoot;
            WindowHost.ElementSetString(handle, "Orientation",
                UIEnumConverter.OrientationText(wrap.Orientation));
            WindowHost.ElementSetNumber(handle, "ItemWidth", wrap.ItemWidth);
            WindowHost.ElementSetNumber(handle, "ItemHeight", wrap.ItemHeight);
            SyncOwnBrush(handle, "Background", wrap, Panel.BackgroundProperty);
        } else if (typeName == "ScrollView") {
            ScrollView scroll = (ScrollView)arcRoot;
            WindowHost.ElementSetString(handle, "HorizontalScrollBarVisibility",
                UIEnumConverter.ScrollBarVisibilityText(scroll.HorizontalScrollBarVisibility));
            WindowHost.ElementSetString(handle, "VerticalScrollBarVisibility",
                UIEnumConverter.ScrollBarVisibilityText(scroll.VerticalScrollBarVisibility));
            WindowHost.ElementSetNumber(handle, "HorizontalOffset", scroll.HorizontalOffset);
            WindowHost.ElementSetNumber(handle, "VerticalOffset", scroll.VerticalOffset);
            WindowHost.ElementSetNumber(handle, "ExtentWidth", scroll.ExtentWidth);
            WindowHost.ElementSetNumber(handle, "ExtentHeight", scroll.ExtentHeight);
            WindowHost.ElementSetNumber(handle, "ViewportWidth", scroll.ViewportWidth);
            WindowHost.ElementSetNumber(handle, "ViewportHeight", scroll.ViewportHeight);
            SyncOwnBrush(handle, "Background", scroll, Panel.BackgroundProperty);
            ScrollRouter.RegisterScrollView(handle, scroll);
        } else if (typeName == "VisualHost") {
            VisualHost host = (VisualHost)arcRoot;
            SyncOwnBrush(handle, "Background", host, Control.BackgroundProperty);
        } else if (typeName == "TabControl") {
            TabControl tabs = (TabControl)arcRoot;
            SyncOwnBrush(handle, "Background", tabs, Panel.BackgroundProperty);
            tabs.BindPlatformMirror(handle);
            PointerRouter.RegisterTabControl(handle, tabs);
        } else if (typeName == "TabItem") {
            TabItem tab = (TabItem)arcRoot;
            SyncOwnBrush(handle, "Background", tab, Panel.BackgroundProperty);
            WindowHost.ElementSetString(handle, "Header", tab.Header);
        } else if (typeName == "TextBlock") {
            TextBlock text = (TextBlock)arcRoot;
            WindowHost.ElementSetString(handle, "Text", text.Text);
            WindowHost.ElementSetNumber(handle, "FontSize", text.FontSize);
            WindowHost.ElementSetString(handle, "FontFamily", text.FontFamily);
            WindowHost.ElementSetString(handle, "FontWeight", text.FontWeight);
            SyncOwnBrush(handle, "Background", text, Control.BackgroundProperty);
            SyncAmbientForeground(handle, text);
            int textEnabled = text.IsEnabled ? 1 : 0;
            WindowHost.ElementSetBool(handle, "IsEnabled", textEnabled);
            // ItemIndex：VirtualizingStackPanel 物化的项行（点击行命中/选中高亮定位用）；
            // 非项行 Text 读回 -1（无害默认）。
            double itemIndex = text.GetAttachedNumber(VirtualizingStackPanel.ItemIndexKey, -1.0);
            WindowHost.ElementSetNumber(handle, "ItemIndex", itemIndex);
        } else if (typeName == "Button") {
            Button button = (Button)arcRoot;
            WindowHost.ElementSetString(handle, "Content", ContentHelper.TextOrEmpty(button.Content));
            WindowHost.ElementSetNumber(handle, "FontSize", button.FontSize);
            WindowHost.ElementSetString(handle, "FontFamily", button.FontFamily);
            WindowHost.ElementSetString(handle, "FontWeight", button.FontWeight);
            SyncOwnBrush(handle, "Background", button, Control.BackgroundProperty);
            SyncAmbientForeground(handle, button);
            int btnEnabled = button.IsEnabled ? 1 : 0;
            WindowHost.ElementSetBool(handle, "IsEnabled", btnEnabled);
            WindowHost.ElementSetBool(handle, "IsMouseOver", 0);
            WindowHost.ElementSetBool(handle, "IsPressed", 0);
            string styleKeys = button.AppliedStyleKeys;
            if (styleKeys == null) {
                styleKeys = "";
            }
            WindowHost.ElementSetString(handle, "StyleKeys", styleKeys);
            Thickness btnPad = Thickness.Parse(button.Padding).Sanitized();
            double padX = btnPad.Left + btnPad.Right;
            double padY = btnPad.Top + btnPad.Bottom;
            if (button.Padding == null || button.Padding == "" || button.Padding == "0,0,0,0") {
                padX = ControlMetrics.ButtonPaddingX;
                padY = ControlMetrics.ButtonPaddingY;
            }
            WindowHost.ElementSetNumber(handle, "PaddingX", padX);
            WindowHost.ElementSetNumber(handle, "PaddingY", padY);
            PointerRouter.RegisterButton(handle, button);
        } else if (typeName == "ToggleButton") {
            ToggleButton toggle = (ToggleButton)arcRoot;
            WindowHost.ElementSetString(handle, "Content", ContentHelper.TextOrEmpty(toggle.Content));
            WindowHost.ElementSetNumber(handle, "FontSize", toggle.FontSize);
            WindowHost.ElementSetString(handle, "FontFamily", toggle.FontFamily);
            WindowHost.ElementSetString(handle, "FontWeight", toggle.FontWeight);
            SyncOwnBrush(handle, "Background", toggle, Control.BackgroundProperty);
            SyncAmbientForeground(handle, toggle);
            int toggleEnabled = toggle.IsEnabled ? 1 : 0;
            WindowHost.ElementSetBool(handle, "IsEnabled", toggleEnabled);
            int toggleChecked = toggle.IsChecked ? 1 : 0;
            WindowHost.ElementSetBool(handle, "IsChecked", toggleChecked);
            WindowHost.ElementSetBool(handle, "IsMouseOver", 0);
            WindowHost.ElementSetBool(handle, "IsPressed", 0);
            PointerRouter.RegisterToggle(handle, toggle);
        } else if (typeName == "CheckBox") {
            ToggleButton checkbox = (ToggleButton)arcRoot;
            WindowHost.ElementSetString(handle, "Content", ContentHelper.TextOrEmpty(checkbox.Content));
            WindowHost.ElementSetNumber(handle, "FontSize", checkbox.FontSize);
            WindowHost.ElementSetString(handle, "FontFamily", checkbox.FontFamily);
            WindowHost.ElementSetString(handle, "FontWeight", checkbox.FontWeight);
            SyncOwnBrush(handle, "Background", checkbox, Control.BackgroundProperty);
            SyncAmbientForeground(handle, checkbox);
            int cbEnabled = checkbox.IsEnabled ? 1 : 0;
            WindowHost.ElementSetBool(handle, "IsEnabled", cbEnabled);
            int cbChecked = checkbox.IsChecked ? 1 : 0;
            WindowHost.ElementSetBool(handle, "IsChecked", cbChecked);
            WindowHost.ElementSetBool(handle, "IsMouseOver", 0);
            WindowHost.ElementSetBool(handle, "IsPressed", 0);
            PointerRouter.RegisterToggle(handle, checkbox);
        } else if (typeName == "RadioButton") {
            RadioButton radio = (RadioButton)arcRoot;
            WindowHost.ElementSetString(handle, "Content", ContentHelper.TextOrEmpty(radio.Content));
            WindowHost.ElementSetNumber(handle, "FontSize", radio.FontSize);
            WindowHost.ElementSetString(handle, "FontFamily", radio.FontFamily);
            WindowHost.ElementSetString(handle, "FontWeight", radio.FontWeight);
            SyncOwnBrush(handle, "Background", radio, Control.BackgroundProperty);
            SyncAmbientForeground(handle, radio);
            int radioEnabled = radio.IsEnabled ? 1 : 0;
            WindowHost.ElementSetBool(handle, "IsEnabled", radioEnabled);
            int radioChecked = radio.IsChecked ? 1 : 0;
            WindowHost.ElementSetBool(handle, "IsChecked", radioChecked);
            WindowHost.ElementSetBool(handle, "IsMouseOver", 0);
            WindowHost.ElementSetBool(handle, "IsPressed", 0);
            WindowHost.ElementSetString(handle, "GroupName", radio.GroupName);
            PointerRouter.RegisterToggle(handle, radio);
        } else if (typeName == "TextBox") {
            TextBox input = (TextBox)arcRoot;
            WindowHost.ElementSetString(handle, "Text", input.Text);
            WindowHost.ElementSetString(handle, "CompositionText", input.CompositionText);
            WindowHost.ElementSetString(handle, "Placeholder", input.Placeholder);
            WindowHost.ElementSetNumber(handle, "FontSize", input.FontSize);
            WindowHost.ElementSetString(handle, "FontFamily", input.FontFamily);
            WindowHost.ElementSetString(handle, "FontWeight", input.FontWeight);
            SyncOwnBrush(handle, "Background", input, Control.BackgroundProperty);
            SyncAmbientForeground(handle, input);
            int readOnly = input.IsReadOnly ? 1 : 0;
            WindowHost.ElementSetBool(handle, "IsReadOnly", readOnly);
            int inputEnabled = input.IsEnabled ? 1 : 0;
            WindowHost.ElementSetBool(handle, "IsEnabled", inputEnabled);
            WindowHost.ElementSetArcPtr(handle, input);
        } else if (typeName == "PasswordBox") {
            // 镜像 Text = 掩码（GeometryText）；明文仅内核 / Password 属性面。
            PasswordBox password = (PasswordBox)arcRoot;
            WindowHost.ElementSetString(handle, "Text", password.GeometryText());
            WindowHost.ElementSetString(handle, "CompositionText", "");
            WindowHost.ElementSetString(handle, "Placeholder", password.Placeholder);
            WindowHost.ElementSetString(handle, "PasswordChar", password.PasswordChar);
            WindowHost.ElementSetNumber(handle, "FontSize", password.FontSize);
            WindowHost.ElementSetString(handle, "FontFamily", password.FontFamily);
            WindowHost.ElementSetString(handle, "FontWeight", password.FontWeight);
            SyncOwnBrush(handle, "Background", password, Control.BackgroundProperty);
            SyncAmbientForeground(handle, password);
            int pwReadOnly = password.IsReadOnly ? 1 : 0;
            WindowHost.ElementSetBool(handle, "IsReadOnly", pwReadOnly);
            int pwEnabled = password.IsEnabled ? 1 : 0;
            WindowHost.ElementSetBool(handle, "IsEnabled", pwEnabled);
            WindowHost.ElementSetArcPtr(handle, password);
        } else if (typeName == "Border") {
            Border border = (Border)arcRoot;
            SyncOwnBrush(handle, "Background", border, Control.BackgroundProperty);
            SyncOwnBrush(handle, "BorderBrush", border, Border.BorderBrushProperty);
            Thickness bt = Thickness.Parse(border.BorderThickness).Sanitized();
            double uniformBt = bt.Left;
            if (bt.Top > uniformBt) { uniformBt = bt.Top; }
            if (bt.Right > uniformBt) { uniformBt = bt.Right; }
            if (bt.Bottom > uniformBt) { uniformBt = bt.Bottom; }
            WindowHost.ElementSetNumber(handle, "BorderThicknessUniform", uniformBt);
            WindowHost.ElementSetString(handle, "BorderThickness", border.BorderThickness);
            WindowHost.ElementSetNumber(handle, "CornerRadius", border.CornerRadius);
            WindowHost.ElementSetString(handle, "Padding", border.Padding);
        } else if (typeName == "Rectangle") {
            Rectangle rect = (Rectangle)arcRoot;
            WindowHost.ElementSetNumber(handle, "Width", rect.Width);
            WindowHost.ElementSetNumber(handle, "Height", rect.Height);
            WindowHost.ElementSetString(handle, "Fill", rect.Fill);
            WindowHost.ElementSetString(handle, "Stroke", rect.Stroke);
            WindowHost.ElementSetNumber(handle, "StrokeThickness", rect.StrokeThickness);
            WindowHost.ElementSetNumber(handle, "RadiusX", rect.RadiusX);
            WindowHost.ElementSetNumber(handle, "RadiusY", rect.RadiusY);
        } else if (typeName == "Slider") {
            Slider slider = (Slider)arcRoot;
            WindowHost.ElementSetNumber(handle, "Value", slider.Value);
            WindowHost.ElementSetNumber(handle, "Minimum", slider.Minimum);
            WindowHost.ElementSetNumber(handle, "Maximum", slider.Maximum);
            WindowHost.ElementSetNumber(handle, "Step", slider.Step);
            SyncOwnBrush(handle, "Background", slider, Control.BackgroundProperty);
            SyncAmbientForeground(handle, slider);
            int sliderEnabled = slider.IsEnabled ? 1 : 0;
            WindowHost.ElementSetBool(handle, "IsEnabled", sliderEnabled);
            WindowHost.ElementSetBool(handle, "IsMouseOver", 0);
            WindowHost.ElementSetBool(handle, "IsPressed", 0);
            PointerRouter.RegisterSlider(handle, slider);
        } else if (typeName == "ProgressBar") {
            ProgressBar progress = (ProgressBar)arcRoot;
            WindowHost.ElementSetNumber(handle, "Value", progress.Value);
            WindowHost.ElementSetNumber(handle, "Minimum", progress.Minimum);
            WindowHost.ElementSetNumber(handle, "Maximum", progress.Maximum);
            int indeterminate = progress.IsIndeterminate ? 1 : 0;
            WindowHost.ElementSetBool(handle, "IsIndeterminate", indeterminate);
            SyncOwnBrush(handle, "Background", progress, Control.BackgroundProperty);
            int progressEnabled = progress.IsEnabled ? 1 : 0;
            WindowHost.ElementSetBool(handle, "IsEnabled", progressEnabled);
            progress.BindPlatformMirror(handle);
        } else if (typeName == "ListView") {
            ListView listView = (ListView)arcRoot;
            WindowHost.ElementSetNumber(handle, "SelectedIndex", (double)listView.SelectedIndex);
            WindowHost.ElementSetNumber(handle, "LayoutHeight", listView.RenderHeight);
            SyncOwnBrush(handle, "Background", listView, Control.BackgroundProperty);
            listView.BindPlatformMirror(handle);
            PointerRouter.RegisterListView(handle, listView);
            // ListView 非 InputElement：显式 Tab 停靠，供 Up/Down/Home/End/Enter 选中。
            FocusManager.RegisterTabStop(listView, handle);
        } else if (typeName == "DataGrid") {
            // RFC 037 §4 · M-VZ4：grid 镜像携带列元数据 + 行区几何 + 选中态；
            // 行镜像（DataGridRow 子元素）由通用递归 + 下方 DataGridRow 分支物化。
            DataGrid dataGrid = (DataGrid)arcRoot;
            WindowHost.ElementSetNumber(handle, "SelectedIndex", (double)dataGrid.SelectedIndex);
            WindowHost.ElementSetNumber(handle, "ColumnCount", (double)dataGrid.ColumnCount);
            WindowHost.ElementSetNumber(handle, "HeaderHeight", dataGrid.HeaderHeight);
            WindowHost.ElementSetNumber(handle, "RowCount", (double)dataGrid.RowCount);
            int colIdx = 0;
            int colCount = dataGrid.ColumnCount;
            while (colIdx < colCount) {
                WindowHost.ElementSetString(handle, "Header" + colIdx, dataGrid.GetColumnHeader(colIdx));
                WindowHost.ElementSetNumber(handle, "Width" + colIdx, dataGrid.GetColumnWidth(colIdx));
                colIdx++;
            }
            dataGrid.BindPlatformMirror(handle);
            PointerRouter.RegisterDataGrid(handle, dataGrid);
        } else if (typeName == "DataGridRow") {
            // 行镜像：ItemIndex（命中测试）+ C{i} 单元格（wgpu 渲染）；
            // 单元格单一来源是父 DataGrid.GetCell（行上不缓存 List）。
            // Layout* 由尾部 FrameworkElement 通用同步写入。
            DataGridRow dataGridRow = (DataGridRow)arcRoot;
            WindowHost.ElementSetNumber(handle, "ItemIndex", (double)dataGridRow.RowIndex);
            if (dataGridRow.Parent is DataGrid) {
                DataGrid parentGrid = (DataGrid)dataGridRow.Parent;
                int cellIdx = 0;
                int cellCount = parentGrid.ColumnCount;
                while (cellIdx < cellCount) {
                    WindowHost.ElementSetString(
                        handle, "C" + cellIdx, parentGrid.GetCell(dataGridRow.RowIndex, cellIdx));
                    cellIdx++;
                }
            }
        } else if (typeName == "ComboBox") {
            // ComboBox<T> 泛型派生自非泛型 ComboBoxBase——选中态与字体面经非泛型
            // 基座读取，无需感知 T；选中变化由 SyncMirrorSelection 增量推送。
            ComboBoxBase combo = (ComboBoxBase)arcRoot;
            WindowHost.ElementSetNumber(handle, "SelectedIndex", (double)combo.SelectedIndex);
            WindowHost.ElementSetString(handle, "SelectedText", combo.SelectedText);
            WindowHost.ElementSetNumber(handle, "FontSize", combo.FontSize);
            WindowHost.ElementSetString(handle, "FontFamily", combo.FontFamily);
            WindowHost.ElementSetString(handle, "FontWeight", combo.FontWeight);
            SyncOwnBrush(handle, "Background", combo, Control.BackgroundProperty);
            SyncAmbientForeground(handle, combo);
            int comboEnabled = combo.IsEnabled ? 1 : 0;
            WindowHost.ElementSetBool(handle, "IsEnabled", comboEnabled);
            combo.BindPlatformMirror(handle);
            PointerRouter.RegisterComboBox(handle, combo);
        } else if (typeName == "Image") {
            // RFC 037 M3.5 + RFC 029 M2：Source/Stretch 写镜像 handle；BindPlatformMirror
            // 回写 TextureId（解码纹理经组件上传后渲染端据此 DrawTexture 采样）。
            // Width/Height 由 FrameworkElement 继承；Background 由 Control 继承。
            Image image = (Image)arcRoot;
            WindowHost.ElementSetString(handle, "Source", image.Source);
            WindowHost.ElementSetString(handle, "Stretch", UIEnumConverter.StretchText(image.Stretch));
            WindowHost.ElementSetNumber(handle, "Width", image.Width);
            WindowHost.ElementSetNumber(handle, "Height", image.Height);
            SyncOwnBrush(handle, "Background", image, Control.BackgroundProperty);
            image.BindPlatformMirror(handle);
        } else if (typeName == "VideoSurface") {
            // RFC 037 references/texture-surface：TextureId 写镜像 handle，wgpu 渲染据此 DrawTexture。
            VideoSurface vs = (VideoSurface)arcRoot;
            WindowHost.ElementSetNumber(handle, "TextureId", (double)vs.TextureId);
            WindowHost.ElementSetString(handle, "Stretch", UIEnumConverter.StretchText(vs.Stretch));
            SyncOwnBrush(handle, "Background", vs, Control.BackgroundProperty);
        } else if (typeName == "CodeEditor") {
            // RFC 037 §4 M-CE1：经 IFrameDrawListProvider 登记（Core 不引用 Edit 包）。
            // 字体/背景走 Control 公共面；行文本由 RenderTree ExecuteDrawList 上屏。
            Control editor = (Control)arcRoot;
            WindowHost.ElementSetNumber(handle, "FontSize", editor.FontSize);
            WindowHost.ElementSetString(handle, "FontFamily", editor.FontFamily);
            WindowHost.ElementSetString(handle, "FontWeight", editor.FontWeight);
            SyncOwnBrush(handle, "Background", editor, Control.BackgroundProperty);
            SyncAmbientForeground(handle, editor);
            if (arcRoot is IFrameDrawListProvider) {
                IFrameDrawListProvider provider = (IFrameDrawListProvider)arcRoot;
                provider.BindDrawListMirror(handle);
            }
        }

        FrameworkElement fe = (FrameworkElement)arcRoot;
        WindowHost.ElementSetNumber(handle, "LayoutX", fe.LayoutX);
        WindowHost.ElementSetNumber(handle, "LayoutY", fe.LayoutY);
        WindowHost.ElementSetNumber(handle, "LayoutWidth", fe.RenderWidth);
        WindowHost.ElementSetNumber(handle, "LayoutHeight", fe.RenderHeight);

        // M-focus 闭环（RFC 037 附录 §2/§4）：InputElement 统一镜像登记 + Tab
        // 停靠注册——基类 ctor 默认 Focusable+IsTabStop（容器型 Window/UserControl/
        // Page 显式 IsTabStop=false 非停靠）；注册顺序 = DFS 前序 = Tab 循环顺序。
        // 此前 RegisterTabStop 无调用方（RFC 037 挂账），Tab 循环惰性；此处接线后
        // Window.Show 的 PrepareForShow 已先 Reset+Install，BuildFromArc 逐个登记。
        if (arcRoot is InputElement) {
            InputElement inputEl = (InputElement)arcRoot;
            inputEl.BindPlatformMirror(handle);
            FocusManager.RegisterTabStop((Control)arcRoot, handle);
        }

        long nextChromeHost = chromeHostHandle;
        if (arcRoot is Control) {
            Control ctl = (Control)arcRoot;
            if (ctl.Template != null) {
                nextChromeHost = handle;
            }
        }

        if (arcRoot.Children != null) {
            for (int i = 0; i < arcRoot.Children.Count; i++) {
                Element child = arcRoot.Children[i];
                long childHandle = PlatformTreeSync.BuildFromArcCore(child, nextChromeHost);
                WindowHost.ElementAddChild(handle, childHandle);
                PlatformTreeSync.StampChromePart(child, childHandle, nextChromeHost);
            }
        }
        return handle;
    }

    /// <summary>模板部件镜像：ChromeHostHandle + ChromeRole（Surface/Glyph/Content）。</summary>
    static void StampChromePart(Element child, long childHandle, long chromeHost) {
        if (chromeHost == 0 || child == null || childHandle == 0) {
            return;
        }
        string name = child.Name;
        if (name == null || name.Length == 0) {
            return;
        }
        string role = "";
        if (name == DefaultControlTemplates.PartChrome) {
            role = "Surface";
        } else if (name == DefaultControlTemplates.PartGlyph) {
            role = "Glyph";
        } else if (name == DefaultControlTemplates.PartContent) {
            role = "Content";
        } else {
            return;
        }
        WindowHost.ElementSetNumber(childHandle, "ChromeHostHandle", (double)chromeHost);
        WindowHost.ElementSetString(childHandle, "ChromeRole", role);
        WindowHost.ElementSetString(childHandle, "Name", name);
    }

    /// <summary>滚轮等运行时事件后：将 Arc 布局坐标/Offset 写回既有平台镜像（不重建树）。</summary>
    internal static void SyncLayoutFromArc(Element arcRoot, long platformRoot) {
        if (arcRoot == null || platformRoot == 0) {
            return;
        }
        SyncLayoutNode(arcRoot, platformRoot);
    }

    static void SyncLayoutNode(Element arcRoot, long platformHandle) {
        FrameworkElement fe = (FrameworkElement)arcRoot;
        WindowHost.ElementSetNumber(platformHandle, "LayoutX", fe.LayoutX);
        WindowHost.ElementSetNumber(platformHandle, "LayoutY", fe.LayoutY);
        WindowHost.ElementSetNumber(platformHandle, "LayoutWidth", fe.RenderWidth);
        WindowHost.ElementSetNumber(platformHandle, "LayoutHeight", fe.RenderHeight);

        string typeName = arcRoot.TypeName;
        if (typeName == "ScrollView") {
            ScrollView scroll = (ScrollView)arcRoot;
            WindowHost.ElementSetNumber(platformHandle, "HorizontalOffset", scroll.HorizontalOffset);
            WindowHost.ElementSetNumber(platformHandle, "VerticalOffset", scroll.VerticalOffset);
            WindowHost.ElementSetNumber(platformHandle, "ExtentWidth", scroll.ExtentWidth);
            WindowHost.ElementSetNumber(platformHandle, "ExtentHeight", scroll.ExtentHeight);
            WindowHost.ElementSetNumber(platformHandle, "ViewportWidth", scroll.ViewportWidth);
            WindowHost.ElementSetNumber(platformHandle, "ViewportHeight", scroll.ViewportHeight);
        }

        if (arcRoot.Children == null) {
            return;
        }
        int count = arcRoot.Children.Count;
        for (int i = 0; i < count; i++) {
            Element child = arcRoot.Children[i];
            long childHandle = WindowHost.ElementGetChild(platformHandle, i);
            if (childHandle != 0) {
                SyncLayoutNode(child, childHandle);
            }
        }
    }
}
