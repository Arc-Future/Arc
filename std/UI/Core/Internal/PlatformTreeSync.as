// RFC 037 M3.6 · RFC 037 Internal: Arc 逻辑树 → 平台 RtUiElement 树一次性同步。
//
// 按 TypeName 分派属性；Content 文本经 ContentHelper.TextOrEmpty 从 Content DP 读取。

namespace Arc.UI.Internal;

using Arc.UI;
using Arc.UI.Components;
using Arc.UI.Components.Layout;
using Arc.UI.Layout;

internal class PlatformTreeSync {
    /// <summary>
    /// 窗口主树代际计数：Window 重建平台主树时自增。弹层等附加层据
    /// 「builtEpoch != RootEpoch」判定旧镜像句柄悬空（跨会话句柄号可被新树
    /// 回收复用）并重走建树路径，见 Popup 文件头诚实边界。
    /// </summary>
    internal static int RootEpoch = 0;

    private PlatformTreeSync() {
    }

    /// <summary>从 Arc 逻辑树根递归构建平台 RtUiElement 树并返回根句柄。</summary>
    internal static long BuildFromArc(Element arcRoot) {
        string typeName = arcRoot.TypeName;
        if (typeName == null || typeName == "") {
            typeName = "Element";
        }
        long handle = WindowHost.ElementCreate(typeName);

        if (typeName == "Window") {
            Window window = (Window)arcRoot;
            WindowHost.ElementSetString(handle, "Background", window.Background);
        } else if (typeName == "StackPanel") {
            StackPanel panel = (StackPanel)arcRoot;
            WindowHost.ElementSetString(handle, "Orientation",
                UIEnumConverter.OrientationText(panel.Orientation));
            WindowHost.ElementSetNumber(handle, "Spacing", panel.Spacing);
            WindowHost.ElementSetString(handle, "Background", panel.Background);
        } else if (typeName == "Grid") {
            Grid grid = (Grid)arcRoot;
            WindowHost.ElementSetNumber(handle, "ColumnSpacing", grid.ColumnSpacing);
            WindowHost.ElementSetNumber(handle, "RowSpacing", grid.RowSpacing);
            WindowHost.ElementSetString(handle, "Background", grid.Background);
        } else if (typeName == "Canvas") {
            Canvas canvas = (Canvas)arcRoot;
            WindowHost.ElementSetString(handle, "Background", canvas.Background);
        } else if (typeName == "DockPanel") {
            DockPanel dock = (DockPanel)arcRoot;
            int lastFill = dock.LastChildFill ? 1 : 0;
            WindowHost.ElementSetBool(handle, "LastChildFill", lastFill);
            WindowHost.ElementSetString(handle, "Background", dock.Background);
        } else if (typeName == "WrapPanel") {
            WrapPanel wrap = (WrapPanel)arcRoot;
            WindowHost.ElementSetString(handle, "Orientation",
                UIEnumConverter.OrientationText(wrap.Orientation));
            WindowHost.ElementSetNumber(handle, "ItemWidth", wrap.ItemWidth);
            WindowHost.ElementSetNumber(handle, "ItemHeight", wrap.ItemHeight);
            WindowHost.ElementSetString(handle, "Background", wrap.Background);
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
            WindowHost.ElementSetString(handle, "Background", scroll.Background);
            ScrollRouter.RegisterScrollView(handle, scroll);
        } else if (typeName == "VisualHost") {
            VisualHost host = (VisualHost)arcRoot;
            WindowHost.ElementSetString(handle, "Background", host.Background);
        } else if (typeName == "TextBlock") {
            TextBlock text = (TextBlock)arcRoot;
            WindowHost.ElementSetString(handle, "Text", text.Text);
            WindowHost.ElementSetNumber(handle, "FontSize", text.FontSize);
            WindowHost.ElementSetString(handle, "FontFamily", text.FontFamily);
            WindowHost.ElementSetString(handle, "FontWeight", text.FontWeight);
            WindowHost.ElementSetString(handle, "Background", text.Background);
            WindowHost.ElementSetString(handle, "Foreground", text.Foreground);
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
            WindowHost.ElementSetString(handle, "Background", button.Background);
            WindowHost.ElementSetString(handle, "Foreground", button.Foreground);
            int btnEnabled = button.IsEnabled ? 1 : 0;
            WindowHost.ElementSetBool(handle, "IsEnabled", btnEnabled);
            WindowHost.ElementSetBool(handle, "IsMouseOver", 0);
            WindowHost.ElementSetBool(handle, "IsPressed", 0);
            PointerRouter.RegisterButton(handle, button);
        } else if (typeName == "ToggleButton") {
            ToggleButton toggle = (ToggleButton)arcRoot;
            WindowHost.ElementSetString(handle, "Content", ContentHelper.TextOrEmpty(toggle.Content));
            WindowHost.ElementSetNumber(handle, "FontSize", toggle.FontSize);
            WindowHost.ElementSetString(handle, "FontFamily", toggle.FontFamily);
            WindowHost.ElementSetString(handle, "FontWeight", toggle.FontWeight);
            WindowHost.ElementSetString(handle, "Background", toggle.Background);
            WindowHost.ElementSetString(handle, "Foreground", toggle.Foreground);
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
            WindowHost.ElementSetString(handle, "Background", checkbox.Background);
            WindowHost.ElementSetString(handle, "Foreground", checkbox.Foreground);
            int cbEnabled = checkbox.IsEnabled ? 1 : 0;
            WindowHost.ElementSetBool(handle, "IsEnabled", cbEnabled);
            int cbChecked = checkbox.IsChecked ? 1 : 0;
            WindowHost.ElementSetBool(handle, "IsChecked", cbChecked);
            WindowHost.ElementSetBool(handle, "IsMouseOver", 0);
            WindowHost.ElementSetBool(handle, "IsPressed", 0);
            PointerRouter.RegisterToggle(handle, checkbox);
        } else if (typeName == "TextBox") {
            TextBox input = (TextBox)arcRoot;
            WindowHost.ElementSetString(handle, "Text", input.Text);
            WindowHost.ElementSetString(handle, "CompositionText", input.CompositionText);
            WindowHost.ElementSetString(handle, "Placeholder", input.Placeholder);
            WindowHost.ElementSetNumber(handle, "FontSize", input.FontSize);
            WindowHost.ElementSetString(handle, "FontFamily", input.FontFamily);
            WindowHost.ElementSetString(handle, "FontWeight", input.FontWeight);
            WindowHost.ElementSetString(handle, "Background", input.Background);
            WindowHost.ElementSetString(handle, "Foreground", input.Foreground);
            int readOnly = input.IsReadOnly ? 1 : 0;
            WindowHost.ElementSetBool(handle, "IsReadOnly", readOnly);
            int inputEnabled = input.IsEnabled ? 1 : 0;
            WindowHost.ElementSetBool(handle, "IsEnabled", inputEnabled);
            WindowHost.ElementSetArcPtr(handle, input);
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
            WindowHost.ElementSetString(handle, "Background", slider.Background);
            WindowHost.ElementSetString(handle, "Foreground", slider.Foreground);
            int sliderEnabled = slider.IsEnabled ? 1 : 0;
            WindowHost.ElementSetBool(handle, "IsEnabled", sliderEnabled);
            PointerRouter.RegisterSlider(handle, slider);
        } else if (typeName == "ListView") {
            ListView listView = (ListView)arcRoot;
            WindowHost.ElementSetNumber(handle, "SelectedIndex", (double)listView.SelectedIndex);
            WindowHost.ElementSetNumber(handle, "LayoutHeight", listView.RenderHeight);
            WindowHost.ElementSetString(handle, "Background", listView.Background);
            listView.BindPlatformMirror(handle);
            PointerRouter.RegisterListView(handle, listView);
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
            // Layout* 由尾部 FrameworkElement 通用同步写入。
            DataGridRow dataGridRow = (DataGridRow)arcRoot;
            WindowHost.ElementSetNumber(handle, "ItemIndex", (double)dataGridRow.RowIndex);
            int cellIdx = 0;
            int cellCount = dataGridRow.Cells.Count;
            while (cellIdx < cellCount) {
                WindowHost.ElementSetString(handle, "C" + cellIdx, dataGridRow.Cells[cellIdx]);
                cellIdx++;
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
            WindowHost.ElementSetString(handle, "Background", combo.Background);
            WindowHost.ElementSetString(handle, "Foreground", combo.Foreground);
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
            WindowHost.ElementSetString(handle, "Background", image.Background);
            image.BindPlatformMirror(handle);
        } else if (typeName == "VideoSurface") {
            // RFC 037 references/texture-surface：TextureId 写镜像 handle，wgpu 渲染据此 DrawTexture。
            VideoSurface vs = (VideoSurface)arcRoot;
            WindowHost.ElementSetNumber(handle, "TextureId", (double)vs.TextureId);
            WindowHost.ElementSetString(handle, "Stretch", UIEnumConverter.StretchText(vs.Stretch));
            WindowHost.ElementSetString(handle, "Background", vs.Background);
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

        for (int i = 0; i < arcRoot.Children.Count; i++) {
            Element child = arcRoot.Children[i];
            long childHandle = BuildFromArc(child);
            WindowHost.ElementAddChild(handle, childHandle);
        }
        return handle;
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
