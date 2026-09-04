# Arc.UI 内置组件

> **视觉立宪**：[RFC 037 §4](../../../docs/rfc/037-ui.md) — 内置控件**默认 Theme 即现代生产级观感**（层次、密度、圆角、色阶、态反馈、焦点可见）；**禁止**「先功能后样子 / 灰框 MVP 终态 / 丑默认 + 美可选双轨」。目标架构见 [RFC 037](../../../docs/rfc/037-ui.md)。**隔离预览宿主**：[RFC 037 §10（VisualHost）](../../../docs/rfc/037-ui.md) — `VisualHost` 内嵌树须自带 Light 默认 Theme，**禁止**以隔离为由无样式灰框。**虚拟化优先**：[RFC 037 §4](../../../docs/rfc/037-ui.md) — 大数据控件**须**视口虚拟化；**标杆** [CodeEditor 虚拟化标杆](#codeeditor--虚拟化立宪m-ce1-硬约束)。**异步调度立宪**：[RFC 037 §7](../../../docs/rfc/037-ui.md)。**公开面 vs 内部面**：[RFC 037 §7](../../../docs/rfc/037-ui.md)。
>
> **状态：Draft** — 本矩阵描述 M3 控件可视/属性面推进进度，**非 Stable 宣称**。
> 图例：✅ 属性注册 + 平台镜像 + wgpu 渲染 + 交互已接 · 🟡 部分（属性/镜像有，渲染或交互缺口）· ⛔ 未实现
>
> **渲染口径（arc-ui 最高优先级规矩）**：渲染**唯一后端 = wgpu**——一切上屏（窗口、控件、文本、滚动条、动画、合成）经 `WgpuRender`（`std/UI/Rendering/Wgpu/`，通过 `crates/arc/native/wgpu-native.ani` 契约直连 wgpu-native：Win→D3D12 / macOS→Metal / Linux→Vulkan）与 `rt_wgpu_native` 完成。软件光栅 `rt_ui_raster.c` / `rt_ui_render_to_buffer`、GDI 文本（`text_gdi.c`）与渲染 stub 已全部删除，**禁止恢复**。
>
> **核对口径（2026-08-15）**：本矩阵逐项对照 `std/UI/Components/`、`std/UI/Markup/`、`std/UI/Core/Internal/PlatformTreeSync.as`（平台镜像）、`std/UI/Rendering/Wgpu/WgpuRender.RenderTree.as`（wgpu 渲染树唯一权威）与 `crates/arc-ui/src/{codegen,typeck}.rs`（ARML 事件/属性面）实据修订；非代码可证项一律不宣称。
>
> **核对增补（2026-08-31）**：内置组件模板让位门禁全量对齐——`WgpuRender.RenderTree` chrome 分支（Button/CheckBox/TextBox/Slider）`templated` 跳过内置 chrome；`TreeDrawListBuilder` 设计时预览同构门禁（已挂子树跳过文本 chrome）；ComboBox 折叠态 chrome 分支落地（提前 `return` 跳过通用递归）并新增矩阵行。三层编写契约见 [production-surface §6](../../../docs/rfc/037-ui/references/production-surface.md)。
>
> **核对增补（2026-08-31 · 选择分层）**：集合类选择语义按 WPF Selector 分层收敛——新建 `Primitives.Selector`（Control → ItemsControl → **Primitives.Selector**）承载单选语义，其上 `Primitives.MultiSelector` 承载多选（SelectionMode/SelectedItems/SelectItem）；ListView/ComboBoxBase（→ ComboBox&lt;T&gt;）派生 Selector，DataGrid 派生 MultiSelector，三份同构选择实现上提为虚钩子差异点，复用通用选中流程（`SelectIndex` 模板方法：校验 → 写点 → 镜像同步 → 附加同步 → 通知）。

## 公开面 vs 内部面

**本矩阵只列应用作者直接使用的控件与布局 API。** 框架接线与 UI 调度器（M-AS1）不在此表——见 [RFC 037 §7](../../../docs/rfc/037-ui.md) 与 `std/UI/Core/Internal/`。**内部 Router 与调度器均非用户 API，调度器亦非用户细控对象。**

| 面 | 示例 | 用户是否直接使用 |
|----|------|------------------|
| **公开面** | `Window` · `Button` · `CodeEditor` · `VisualHost` · 布局面板 · `DrawList` | ✅ |
| **内部面** | 框架接线 · UI 调度器（`std/UI/Core/Internal/`） | ❌ |
| **互操作面** | `WindowHost.NativeHandle` | ⚠️ 原生集成（互操作面） |

| 组件 | DP / ARML 属性面 | 平台镜像 (codegen) | wgpu 渲染（WgpuRender） | 交互 | 虚拟化 |
|------|------------------|-------------------|--------------------------|------|-------------------|
| **Window** | ✅ Title/Left/Top/Content/W/H/Background | ✅ Background | ✅ 根背景 + 子树（`RenderElementTree` 根 `DrawRect` + 通用递归） | 🟡 Show/ShowAsync（帧泵）；Close 已接编程式关闭（codegen 传平台镜像句柄 → `rt_window_close`；真窗 GUI 手测待补） | ⛔ 不适用 |
| **StackPanel** | ✅ Orientation/Spacing/Background/Margin | ✅ Orientation/Spacing/Background | ✅ 背景（`DrawBackground`）+ 子元素递归 | ⛔ 无用户交互语义 | ⛔ 固定少量子项例外 |
| **TextBlock** | ✅ Text/FontSize/Foreground/Background/Font* | ✅ Text/FontSize/Background/Foreground/IsEnabled | ✅ `DrawText` 真实字形（动态 stb_truetype atlas，8x16 点阵 fallback；布局 Measure 与绘制同源 `ITextMetrics`） | ⛔ 字形/选区 M4+ | ⛔ 不适用 |
| **Button** | ✅ Content/Background/Foreground/FontSize/IsEnabled/IsMouseOver/IsPressed/Command/CommandParameter/Click（string）+ **Clicked（Signal）** | ✅ Content/FontSize/Background/Foreground/IsEnabled + PointerRouter | ✅ 软阴影 + 圆角渐变/填充 + 描边 + 焦点外晕 + 文本（`DrawSurfaceShadow`/`DrawLinearGradient`/`DrawRoundedRect`/`DrawRoundedBorder`/`DrawText`；Hover/Pressed/Focus 态色经 VSM + MotionEngine） | 🟡 Click → code-behind（codegen `OnClick(_ => this.X())`）+ 指针 hit（Win32，≤8 槽）+ M-focus Tab/Enter/Space 激活（Win32 Draft） | ⛔ 不适用 |
| **Rectangle** | ✅ W/H/Fill/Stroke/StrokeThickness/RadiusX/Y | ✅ W/H/Fill/Stroke/StrokeThickness/RadiusX/Y | ✅ fill + stroke（`DrawRect` + `DrawRectBorder`；圆角走 `DrawRoundedRect`/`DrawRoundedBorder`） | ⛔ 无交互语义（Shape） | ⛔ 不适用 |
| **ToggleButton** | ✅ IsChecked/IsThreeState/Content/IsEnabled + Checked/Unchecked/Indeterminate（ARML 事件名 string）+ **Toggled（Signal）** | ✅ IsChecked/Content/FontSize/Background/Foreground/IsEnabled + PointerRouter（Toggle 槽 · 点击路由） | ✅ 同 Button 分支 + IsChecked Accent 内框（`DrawRoundedRect`） | 🟡 Toggled 通道（`IsChecked` setter 触发，Signal<bool> + OnToggled）+ **点击切换已接**（PointerRouter 泛化 · `RaiseToggle` 分发）；⛔ 键盘切换未接；⛔ GUI 手测待补 | ⛔ 不适用 |
| **CheckBox** | ✅ 继承 ToggleButton（语义占位类，无额外成员） | ✅ 同 ToggleButton（继承镜像 + Toggle 槽） | ✅ 勾选盒 + 描边 + Accent 内框 + 文本（`DrawRoundedRect`/`DrawRoundedBorder`/`DrawText`） | 🟡 点击切换已接（继承 `RaiseToggle`）；⛔ GUI 手测待补 | ⛔ 不适用 |
| **TextBox** | ✅ Text/Placeholder/IsReadOnly/MaxLength/CaretIndex/CompositionText + **TextChanged（Signal）** | ✅ Text/CompositionText/Placeholder/CaretIndex/IsFocused/IsReadOnly/IsEnabled + ImeBridge/InputFocusRouter | ✅ 圆角填充 + 描边 + 焦点外晕 + 文本 + composition 下划线 + caret 竖线（`DrawRoundedRect`/`DrawRoundedBorder`/`DrawSurfaceShadow`/`DrawText`/`DrawRect`） | 🟡 TextChanged 通道（Text setter 触发）+ M-ime1 Win32 路径已接线（ASCII `WM_CHAR` 直输 + IME commit → Text；§8 中文手测 ☐）+ M-focus Tab 焦点框（Win32 Draft）；编辑内核 TextBoxModel（撤销/重做 + 选区管理 · 无头可测）+ TextBoxController 事件路由 | ⛔ 不适用 |
| **Slider** | ✅ Value/Minimum/Maximum/Step/Background/Foreground + **ValueChanged（Signal）** | ✅ Value/Minimum/Maximum/Step/Background/Foreground/IsEnabled + PointerRouter（Slider 槽 · 拖拽路由） | ✅ track + fill + thumb（`DrawRoundedRect`，Track/Accent 主题色经 VSM）+ **模板让位**（templated 跳过内置 chrome） | 🟡 ValueChanged 通道（`Value` setter 触发，Signal<double> + OnValueChanged）+ **鼠标拖拽/点击已接**（PointerRouter 泛化 · `rt_ui_slider_value_from_px` 像素→值（Step 取整 + clamp）→ `ApplyDragValue` → `SetValue` 触发 ValueChanged）；⛔ GUI 手测待补 | ⛔ 不适用 |
| **ComboBox** | ✅ SelectedIndex/SelectedText（DP 面 · 渲染 chrome 与平台镜像消费）/OptionCount/SelectedValue + SelectIndex/SetOptions(EnumOptions&lt;T&gt; 强类型数据源) + **SelectionChanged（Signal&lt;T&gt;）** | ✅ SelectedIndex/SelectedText/FontSize/FontFamily/FontWeight/Background/Foreground/IsEnabled（经非泛型 `ComboBoxBase` 基座读取，泛型派生零感知 T · `SyncMirrorSelection` 增量推送） | ✅ 折叠态 chrome：圆角填充 + 描边（VSM ComboBox 态色 + Motion）+ SelectedText 文本 + 右侧 chevron（三横条近似）；chrome 分支提前 `return`（选项行不经通用递归，防叠加）；✅ 展开态 Popup 轨已接（chrome 点击 → `RouteChromeClick` → Popup{ListView}：镜像绝对坐标定位 + 静态方法组回调经 `_activeCombo` 互斥槽路由 + 蒙层点击关闭；v1 下拉底色固定白/超窗口底裁剪，见 ComboBox.as 文件头诚实边界）；⛔ 用户 ControlTemplate 让位未接 | 🟡 SelectionChanged 通道（`SelectIndex` 触发，Signal&lt;T&gt; + OnSelectionChanged，与 ListView 同惯用法）+ 下拉展开/选项点击已接（chrome 点击路由 + ListView 命中联动关闭）；⛔ GUI 手测待补 | ⛔ 不适用 |
| **Popup** | ✅ Child/PlacementX/PlacementY + Open/Close/IsOpen + **Closed（Signal&lt;bool&gt;）**（OnClosed 订阅） | ✅ 附加层根独立建树（`BuildFromArc` 挂窗口平台根，非主树子节点）+ `RootEpoch` 代际守卫（宿主重建后句柄悬空自愈重走建树）+ 蒙层背景直写 | ✅ `PopupLayer`/`PopupBackdrop` 分支（层根 + 蒙层置顶绘制） | 🟡 蒙层点击关闭（PointerRouter `PopupBackdrop` 槽）；同窗口至多一个展开由消费方互斥槽（`_activeCombo` 型锚点）保证；⛔ 多弹层叠序/滚动内翻定位/主题化后置 | ⛔ 不适用 |
| **Image** | ✅ Source/Stretch/W/H/Background | ✅ Source/Stretch/Width/Height/Background（PlatformTreeSync.Image 分支 · `Window.Show()` 同步镜像树） | 🟡 占位框（背景 + 描边 `DrawRect`/`DrawRectBorder`）；**Source 解码位图未进 wgpu 路径**（动态纹理走 `VideoSurface` → `DrawTexture` 已接） | ⛔ 显示效果未 GUI 手测验收 | ⛔ 不适用 |
| **ScrollView** | ✅ Content/ScrollBarVisibility（H·V）/H·VOffset + ExtentWidth·Height/ViewportWidth·Height/ScrollableWidth·Height 只读 | ✅ 全属性 + ScrollRouter | ✅ 视口裁剪（scissor）+ 内容直通 + **竖滚动条**（`DrawVScrollBar` 轨道/滑块主题色；几何与 `rt_ui_vscroll_*` 同契约） | 🟡 滚轮/拖拽命中已解封（容器·根节点写 avail 布局 rect → `rt_ui_find_scrollview_at` 可达；C 级 e2e `ui_container_layout_e2e` 覆盖）；`scroll_win32.c` 滚轮 Offset + 拖动 thumb/track 已接；⛔ GUI 手测未过 | 🟡 内容须自带虚拟化（M-VZ1+） |
| **ListView** | ✅ 选择面五 DP + **SelectionChanged（Signal&lt;string&gt;）** + SelectionChangedHandler（string 事件名）均**继承 Primitives.Selector**（覆写 `OnSelectionApplied` 装箱 SelectedItem）；ItemsSource（ItemSourceView 强类型数据面）/ItemTemplate 继承 ItemsControl | ✅ SelectedIndex/LayoutHeight + 行 TextBlock ItemIndex（点击命中/高亮定位）+ PointerRouter（ListView 槽 · 点击路由） | ✅ LayoutShell 背景 + 行 TextBlock 递归（选中行高亮经行 TextBlock 呈现；无专属 wgpu 分支） | 🟡 点击选择已接（PointerRouter 泛化 · C 像素命中行 → 镜像 HitItemIndex → `SelectIndex`（Primitives.Selector）→ 选中行高亮 + SelectionChanged 通道载荷=新选中项显示投影）；⛔ Signal.Subscribe 闭包链路挂账（M-D0）；⛔ GUI 点击手测待补；⛔ 键盘/Multiple 后置 | **必须**视口 + 回收池（M-VZ1+） |
| **ItemsControl** | ✅ ItemsSource（**唯一数据入口**：string/List/ObservableCollection/ItemSourceView 判别物化为强类型视图——object 本体 + 显示投影双通道；null 清空）/ItemTemplate/ItemsPanel/VerticalOffset/ItemHeight（命令式 Set*Items 已撤面，单一惯用法对标 WPF；DisplayMemberPath 已撤，投影并入视图构造期） | ⛔ 无分支 | ✅ LayoutShell 背景（泛化递归） | ⛔ 无（ItemsSource 物化 + 虚拟化非交互面） | **必须**视口 + 回收池（M-VZ1+） |
| **VirtualizingStackPanel** | ✅ VerticalOffset/ItemHeight/CacheLengthBefore/CacheLengthAfter/Orientation | ⛔ 无分支 | ✅ LayoutShell 背景（泛化递归） | ⛔ 无 | **视口窗口** · M-VZ1 · 回收池 |
| **Grid** | ✅ ColumnSpacing/RowSpacing/Background + ColumnDefinitions/RowDefinitions（List&lt;object&gt; 字段，非 DP）+ **Grid.Row/Column 附加属性（typed DependencyProperty&lt;int&gt;）** | ✅ ColumnSpacing/RowSpacing/Background | ✅ LayoutShell 背景（行列算法由 GridLayout 决定布局 rect，渲染只消费权威 rect） | ⛔ 无交互语义 | ⛔ 固定少量子项例外 |
| **DockPanel** | ✅ LastChildFill/Background + Dock 附加属性（string 占位，M3+ RegisterAttached） | ✅ LastChildFill/Background | ✅ LayoutShell 背景（停靠算法由 DockLayout 决定布局 rect） | ⛔ 无交互语义 | ⛔ 固定少量子项例外 |
| **WrapPanel** | ✅ Orientation/ItemWidth/ItemHeight/Background | ✅ Orientation/ItemWidth/ItemHeight/Background | ✅ LayoutShell 背景（换行算法由 Flexbox.ArrangeWrap 决定布局 rect） | ⛔ 无交互语义 | ⛔ 固定少量子项例外 |
| **Canvas** | 🟡 Left/Top/Right/Bottom 附加属性（string 占位，M3+ RegisterAttached）+ Background | ✅ Background | ✅ LayoutShell 背景（绝对定位由 CanvasLayout 决定布局 rect） | ⛔ 无交互语义 | ⛔ 固定少量子项例外 |
| **VisualHost** | ✅ Content/Child/Resources/GetHostResources/SetContent/Rebuild/Clear/Navigate + **ContentChanged/InnerLoaded/InnerUnloaded（Signal）** + IsDataContextBoundary | ✅ Background | ✅ 背景 + 内层根递归（内层根加载/卸载驱动重绘） | 🟡 ContentChanged/InnerLoaded/InnerUnloaded 通道；⛔ M-VH3 输入/焦点/HWND 后置 · **非 1GB 宿主** | 🟡 预览区推荐虚拟化模板 |
| **CodeEditor** | ✅ VerticalOffset/DocumentPath（DP 元数据；字段后备 __sinit 挂账）+ OpenPath/RenderVirtualizedLines/ContentExtentHeight/SetText | ⛔ 无全文平台镜像（无分支） | 🟡 泛化递归（DrawList 视口虚拟化未进 wgpu 分支） | ⛔ IME/选区/LSP 后置 | **标杆** · CodeEditor 行视口 |
| **DataGrid** | ✅ SelectedIndex + **SelectionChanged（Signal&lt;string&gt;）** + SelectionChanged（ARML 事件名 · typeck 标识符校验；codegen 挂账待接，现仅 Button.Click 有先例）**继承 Primitives.MultiSelector**（单选面继承 Primitives.Selector；覆写 `SelectionItemCount` 行数上界 + `SelectionPayload` 行首列文本；构造 `ownsItemsHost=false` 自管行宿主——多列单元格视口与基类项宿主管线正交）+ RowHeight/HeaderHeight/VerticalOffset + 编程式 AddColumn/AddRow/GetCell/ClearRows/SelectIndex | ✅ SelectedIndex/ColumnCount/Header{i}/Width{i}/RowHeight/HeaderHeight/RowCount + 行镜像 ItemIndex/C{i}/Layout*（PlatformTreeSync DataGrid/DataGridRow 分支）+ PointerRouter（DataGrid 槽 · 点击路由） | ✅ 专属分支 `RenderDataGrid`：整格底 + 表头带（Stripe 底 + 列头文本 + 底分隔线）+ 斑马纹（`Color.Surface.Stripe`）+ 选中 Accent 整行 + OnAccent 文本 + 列分隔线 + 外框；单元格列宽省略截断（`ClipTextToWidth`）+ 行区 scissor 裁剪 | 🟡 点击选择已接（C `rt_ui_datagrid_hit_row` 行镜像 layout_y 命中 → HitItemIndex → `SelectIndex`（Primitives.Selector）→ SelectionChanged 载荷=选中行首列文本）；⛔ Signal.Subscribe 闭包链路挂账（M-D0）；⛔ GUI 手测待补；⛔ 列拖拽排序/编辑后置 | **必须**行虚拟化 · M-VZ4 ✅（视口窗口 + 回收池；Extent=rowCount×stride 纯算术；e2e `ui_datagrid_selection_e2e` + C `ui_datagrid_selection_c_e2e`） |
| **TreeView** | ⏳ | ⏳ | ⏳ | ⏳ | **必须**展开路径视口（M-VZ4） |

> \* **SelectedIndex int DP 运行期挂账**：`Signal<int>` 泛型 ABI 首次实例化 AV（`Element.SetValue<int>` 直崩，见 `ui_listview_selection_e2e` 诚实边界）。选择分层后 `Primitives.Selector.SelectedIndex` 统一走 **DP wrapper**（原 ListView/DataGrid 字段后备随迁移消失，DP 元数据与运行期读写同轨）；泛型 int 槽运行期 AV 若在选中流程触发即暴露，届时按 ABI 挂账清偿流程处理。

## 基类与宿主（不入矩阵行 · 供说明）

以下类型为**基类/宿主/内部接线**，不是应用作者直接使用的控件，故不占矩阵行：

| 类型 | 文件 | 说明 |
|------|------|------|
| `Element` | `std/UI/Markup/Element.as` | 逻辑树根：DataContext（inherit DP + 边界）/Name/TypeName/Children/Parent（弱引用）/GetValue·SetValue·Observe/AddChild/OnInitialized·OnLoaded·OnUnloaded/SetAttachedNumber·SetAttachedString/`RegisterDetach`（G2 卸载退订原语 · RFC 027 §5.3） |
| `FrameworkElement` | `std/UI/Markup/FrameworkElement.as` | Width/Height/Min*/Max*/Margin/Alignment/Style/Resources/Tag DP + Measure/Arrange 两阶段（MeasureOverride/ArrangeOverride） |
| `Control` | `std/UI/Markup/Control.as` | Background/Foreground/FontFamily/FontSize/FontWeight/IsEnabled/Template/Focusable/IsTabStop DP |
| `Panel` | `std/UI/Markup/Panel.as` | 布局面板基类：Background DP（复用 Element.Children） |
| `Shape` | `std/UI/Markup/Shape.as` | 图形基类：Fill/Stroke/StrokeThickness DP（Rectangle 父类） |
| `ContentControl` | `std/UI/Components/ContentControl.as` | Content（Content variant）/ContentTemplate/ContentStringFormat/ContentDirection/H·VContentAlignment/Padding DP；Window/Button/ToggleButton/UserControl/Page 父类 |
| `Selector` | `std/UI/Core/Components/Primitives/Selector.as` | 单选语义层（WPF Selector 对标；Control → ItemsControl → **Primitives.Selector** → ListView / ComboBoxBase → ComboBox&lt;T&gt;）：SelectedIndex/SelectedItem/SelectedValue/SelectedValuePath 四 DP + `SelectIndex` 模板方法选中流程（校验 → 写点 → 镜像同步 → 附加同步 → 通知）+ 五 protected virtual 钩子（`SelectionItemCount`/`ApplySelectedIndexCore`/`OnSelectionApplied`/`SelectionPayload`/`RaiseSelectionChanged`）+ SelectionChanged Signal&lt;string&gt;；DataGrid 经 protected ctor `ownsItemsHost=false` 自管行宿主（新单选集合类控件一律派生此层复用选择面，禁止重写选中流程） |
| `MultiSelector` | `std/UI/Core/Components/Primitives/MultiSelector.as` | 多选语义层（WPF MultiSelector 对标；**Primitives.MultiSelector** → DataGrid）：派生 `Primitives.Selector` 承接全单选面 + SelectionMode/SelectedItems/SelectItem 多选扩展（多选语义载体，单选层 Selector 不感知；新多选集合类控件派生此层） |
| `InputElement` | `std/UI/Markup/InputElement.as` | 输入组件公共基类（WPF 同构）：焦点管理 + 键盘路由 + 默认激活（Activate）；Button/TextBox/ContentControl 继承 |
| `TextBoxModel` / `TextBoxController` | `std/UI/Editing/TextBoxModel.as` · `std/UI/Core/Internal/TextBoxController.as` | TextBox 编辑内核（纯逻辑 · 撤销/重做/选区 · 无头可测）+ 平台事件→模型操作路由（internal，不暴露开发者） |
| `ContentPresenter` | `std/UI/Components/ContentPresenter.as` | ControlTemplate 内 Content 呈现占位（OnLoaded 沿父链同步 ContentControl.Content；ApplyTo 显式注入） |
| `UserControl` / `Page` | `std/UI/Components/UserControl.as` · `Page.as` | ContentControl 语义子类（UserControl 空类；Page 有 Title DP） |
| `Application` | `std/UI/Components/Application.as` | MainWindow/Resources + Run·RunAsync/RunCore/OnStartup/OnExit（隐式样式 + IME handler 装配） |
| `ICommand` | `std/UI/Components/ICommand.as` | CanExecute/Execute 接口（Button.Command 预留 · MVVM） |
| `ItemContainerGenerator` | `std/UI/Components/ItemContainerGenerator.as` | ItemsHost 回收池 + 视口物化（M-VZ1；ItemsControl 内部接线，非控件） |
| `RowDefinition` / `ColumnDefinition` | `std/UI/Components/Layout/RowDefinition.as` · `ColumnDefinition.as` | Grid 行/列定义（GridLength.Auto/Star/px） |
| `WindowHost` | `std/UI/Components/WindowHost.as` | 静态 ABI 桥（ElementCreate/Set*/Get*/AddChild/IME/键盘/滚动 handler）；`NativeHandle`/`CreateWindow`/`RunEventLoop` 为互操作面 |

## 默认观感（Light Theme）

> **核对（2026-08-04）**：以下 Token 值与 `std/UI/Styling/DesignTokens.as` / `ThemeDictionary.as` 实据逐项一致（`Color.Focus.Ring` 为 `#661677FF`，即 `#1677FF` @ 40% alpha）。

| Token | 值 | 消费方 |
|-------|-----|--------|
| `Color.Primary` | `#1677FF` | Button 默认填充 |
| `Color.Primary.Hover` | `#4096FF` | Button `:hover` |
| `Color.Primary.Pressed` | `#0958D9` | Button `:pressed` |
| `Color.Surface` | `#FFFFFF` | TextBox 背景 |
| `Color.Border` | `#E8E8E8` | TextBox 描边 |
| `Color.Focus.Ring` | `#1677FF` @ 40% | Button/TextBox `:focus-visible` 外环 |
| `Radius.Control` | `6` | Button / TextBox 圆角（wgpu SDF 圆角） |
| `Spacing.MD` | `12` | Button 水平内边距 |
| `Spacing.SH` | `8` | TextBox 内边距 |

**诚实缺口（本刀）**：`:focus-visible` 依赖 mirror `IsFocused`/`IsFocusVisible` bool（focus 刀合入前静态演示有限）；Image 位图已进双宿主 wgpu 路径（Stretch 映射统一走 `StretchMapper`，WPF 语义）但 **GUI 手测验收待补**；CodeEditor DrawList 视口虚拟化未进 wgpu 分支；ScrollBar/Slider §6.3–6.4 部分达标（Slider 拖拽/点击已接但 **GUI 手测待补**）。

## 依赖接口（与其他「刀」的边界）

| 接口 | 提供方 | 消费方（控件刀） |
|------|--------|-----------------|
| `WindowHost.ElementCreate/Set*/Get*/AddChild` | platform `window.cpp` | codegen 平台镜像 |
| `rt_ui_design_tokens.h` / `DesignTokens.as` | **本刀** | 默认 Theme 常量 |
| `WgpuRender.RenderElementTree` | **渲染刀** · std/UI/Rendering/Wgpu（唯一后端 · `wgpu-native.ani` 契约） | wgpu 路径属性→GPU 图元（D3D12/Metal/Vulkan） |
| `Measure`/`Arrange` 布局 | **布局刀** · std/UI/Layout（Flexbox/GridLayout/DockLayout/CanvasLayout） | StackPanel/Grid/Canvas/DockPanel/WrapPanel 尺寸/对齐 |
| **视口虚拟化 / 容器回收** | **M-VZ 刀** · [RFC 037 §4](../../../docs/rfc/037-ui.md) | ItemsControl/ListView/DataGrid/ScrollView 内容；标杆 [CodeEditor 虚拟化标杆](#codeeditor--虚拟化立宪m-ce1-硬约束) |
| **DrawList 可见项 lowering** | [RFC 037 §4](../../../docs/rfc/037-ui.md) | 与虚拟化联合签收 M-VZ3 |
| 字形/atlas | M4 渲染 | TextBlock/Button 内容绘制 |
| `{x:Bind}` → 镜像 + 重绘 | 绑定刀 · M4+ | 动态属性 |
| `rt_ui_ime_*` | **IME 平台刀** · `crates/runtime/platform/windows/ime_win32.c` 等 | TextBox · `ImeBridge` 消费 commit 队列（M-ime1 Win32 已接线） |
| `rt_editor_*` / `rt_file_mmap_*` | **CodeEditor 刀** · M-CE1 | Piece Table + mmap 打开；**禁止 ReadAllText** |
| `UIDispatcher` / `FramePump` / `Application.RunAsync` | **异步调度刀** · M-AS1 | 长 I/O 后台 + 主线程 Post；**禁止** UI 线程阻塞 ReadAllText；`RunEventLoop` 仅为兼容 |
| `rt_ui_set_button_click_handler` / `rt_ui_set_keyboard_handler` / `rt_ui_set_scroll_wheel_handler` | platform `window.cpp` / `keyboard_win32.c` / `scroll_win32.c` | Button Click · M-focus Tab/Enter/Space · ScrollView 滚轮 |
| `WindowHost.ImeSetFocusRect/ImeTakeCommit/…` | codegen stub → 上列 ABI | `TextBox.UpdateImeFocusRect` / `DrainCommits` |

## CodeEditor · 虚拟化立宪（M-CE1 硬约束）

Arc.UI **优选项**：大列表/大文档控件默认 **视口虚拟化**，禁止为演示方便全量创建每行 `TextBlock`/`Visual` 子元素。

| 规则 | CodeEditor M-CE1 |
|------|------------------|
| 缓冲 | Piece Table（C `rt_editor.c`）+ mmap 原稿；**否决** Rope / 全量 `string` |
| 打开 | `OpenPath` → `rt_file_mmap_*`；**禁止** `File.ReadAllText` |
| 渲染 | `RenderVirtualizedLines()` → DrawList；仅可见行 ± `OverscanLines` |
| Extent | `ContentExtentHeight = LineCount × LineHeight`（算术；ScrollView 不 Measure 全文） |
| 宿主 | 真实 `Window`/`ScrollView`；**VisualHost 仅小文件预览** |

权威：[RFC 037 §4](../../../docs/rfc/037-ui.md) · 短研 `2c4a16f1`。

## 内置控件验收

| 控件 | 路径 | 验收 |
|------|------|--------------|
| Button | `Components/Button.as` | §6.1（M2+） |
| TextBox | `Components/TextBox.as` | §6.2（M2+） |
| ScrollView | `Components/Layout/ScrollView.as` | §6.3（M2+） |
| Slider | `Components/Slider.as` | §6.4（M2+） |
| TextBlock | `Components/TextBlock.as` | 继承 `Font.Body` Token |
| ToggleButton | `Components/ToggleButton.as` | 同 CheckBox 态反馈原则（IsChecked/IsThreeState + Toggled） |
| CheckBox | `Components/CheckBox.as` | 同 Button 态反馈原则 |
| Window | `Components/Window.as` | `Color.Background` 层次 |
| VisualHost | `Components/VisualHost.as` | §8（M-VH1+；隔离区仍须默认 Theme） |

## 虚拟化验收（Draft · M-VZ3+）

| 控件 | 路径 | 指标 |
|------|------|------|
| ListView | `Components/ListView.as`（继承 Primitives.Selector → ItemsControl） | 10 万项 scroll ≥ 60 fps；Visual 数 ≈ O(viewport) |
| ItemsControl / VirtualizingStackPanel | `Components/ItemsControl.as` · `Components/Layout/VirtualizingStackPanel.as` | M-VZ1 视口物化 + 回收池（`ItemContainerGenerator`） |
| ScrollView | `Components/Layout/ScrollView.as` | 大数据 Content 不全量 Measure extent |
| CodeEditor | `Components/CodeEditor.as` | 1GB 文档 scroll；**虚拟化标杆** · M-VZ5 联签 |
| DataGrid | `Components/DataGrid.as` | M-VZ4 ✅：视口窗口 + 回收池 + Extent 纯算术（`ui_datagrid_selection_e2e`：100 行默认视口 last&lt;99；未物化行 `GetCell` 可读）；fps 量化签收待补 |

## 演示

- `examples/ArmlDemo` — 单一综合演示：单 Window + ScrollView 10 分区（Hello 元素树 · Controls 控件面 · x:Bind · ListView · Image · Slider · IME/TextBox · VisualHost 样式隔离 · CodeEditor 视口虚拟化 · DataGrid 行虚拟化表格）；`arc build examples/ArmlDemo`

## 验证

```text
cargo test -p arc-ui
```

> 注：原 `ui_skeleton_honesty_e2e` 已随 `arc-integration` 退场（a2627a0f），
> 骨架证据面由 `crates/arc-ui/tests/` 承接。

渲染快照验收（M2+）：wgpu 默认 Theme，无用户 Style 覆盖。
