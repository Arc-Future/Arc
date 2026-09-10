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
> **核对增补（2026-09-09 · Margin 内容盒 + Button 内容尺寸 + List 行距）**：① `FrameworkElement.Arrange`：外边距盒原点 +Margin → 内容盒 `LayoutX/Y`，`RenderSize`=无 Margin（生产面 §1 禁 Margin 布局忽略；ArmlDemo 页 `Margin="16,12,…"` 左/上内缩生效，消「内容贴边 / 左缘空洞」类观感）；② Button/ToggleButton 构造期 `HorizontalAlignment=Left`（Ant 内容尺寸 chrome，禁竖向 Stack 拉满栏宽；隐式 Style 仍仅 BasedOn Medium）；`LayoutHelper.ButtonPadding*` 字面量对齐 `ControlMetrics`（15/4）；③ VSP 行 arrange 收 `FrameworkElement`（非仅 TextBlock）；`ResolveItemStride` 用 `EstimateLineHeight`+`MinTextPaddingY` 防行重叠。

> **核对增补（2026-09-09 · Data/List/Input 三回归）**：① CodeEditor `ArrangeOverride` 须 `void`（误返 `LayoutSize` 与基类 ABI 不符 → 第 8 tab AV）；DataGrid `SyncMirrorRows` 写绝对 `LayoutX/Y`；② ItemsControl `ArrangeChild` 项宿主 + VSP `VerticalOffset` + ListView `PushClip`（行叠原点）；③ 模板态 TextBox/PasswordBox/ComboBox：**先 PART 子树再内容层并 `return`**（壳盖 caret/placeholder）；`SyncMirrorText` → `Invalidate` 非布局脏。

> **核对增补（2026-09-09 · Placeholder 禁跟 caret 闪 + 聚焦即消）**：① 空+焦点时 placeholder 曾与 caret 共用 `RoleForeground` Motion 槽 → blink 亮/灭切换目标色致水印闪；占位色改 `ResolveThemeKey` 恒定。② **显示条件**：仅 `Text` 空且 **未聚焦** 时画水印；聚焦即消，只留 caret（有文案时亦消）。

> **核对增补（2026-09-09 · chrome 模板化）**：① 默认 `ControlTemplate` = `DefaultControlTemplates` 挂隐式 Style Template Setter（Instantiate 多实例）；② 部件 `PART_Chrome` / `PART_Glyph` / `PART_Content`；③ PlatformTreeSync 写 `ChromeHostHandle`+`ChromeRole`；④ RenderTree 有模板子树跳过已迁宿主 chrome，PART 消费 VSM；TextBox/PasswordBox/ComboBox 壳在 PART_Chrome、文本/caret/chevron 仍宿主层；Slider/ProgressBar 轨填充在 PART Surface；⑤ 无模板宿主分支仅回退，**禁止**为已迁控件新增硬编码 chrome；⑥ **已迁**：Button/Toggle/Check/Radio/TextBox/PasswordBox/Slider/ProgressBar/ComboBox；**未迁**：TabControl（Panel + 内容子树，`templated` 门禁不适用）、DataGrid（`ApplyTo` 清子树毁行镜像；专属 `RenderDataGrid` 过大，诚实后置）。
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
| **Button** | ✅ Content/Background/Foreground/FontSize/IsEnabled/IsMouseOver/IsPressed/Command/CommandParameter/Click（string）+ **Clicked（Signal）**；变体 = **`Style="{StaticResource Primary, Small}"`**（§0.1.1；**禁 Class/Appearance/`*.Size.SM`**）；**无 Appearance DP**；**默认 Template**（PART_Chrome+PART_Content） | ✅ Content/FontSize/Background/Foreground/IsEnabled/StyleKeys/PaddingX\|Y + PointerRouter | ✅ **模板优先**（PART VSM）；无模板宿主回退 | 🟡 Click + 指针 hit + M-focus | ⛔ 不适用 |
| **Rectangle** | ✅ W/H/Fill/Stroke/StrokeThickness/RadiusX/Y | ✅ W/H/Fill/Stroke/StrokeThickness/RadiusX/Y | ✅ fill + stroke（`DrawRect` + `DrawRectBorder`；圆角走 `DrawRoundedRect`/`DrawRoundedBorder`） | ⛔ 无交互语义（Shape） | ⛔ 不适用 |
| **ToggleButton** | ✅ IsChecked/IsThreeState/Content/IsEnabled + Checked/Unchecked/Indeterminate（ARML 事件名 string）+ **Toggled（Signal）**；**默认 Template** | ✅ IsChecked/Content/FontSize/Background/Foreground/IsEnabled + PointerRouter（Toggle 槽） | ✅ **模板优先**（PART_Chrome VSM.Toggle） | ✅ 点击切换 + Enter/Space | ⛔ 不适用 |
| **CheckBox** | ✅ 继承 ToggleButton；**默认 Template**（PART_Glyph+PART_Content） | ✅ 同 ToggleButton | ✅ **模板优先**（PART_Glyph） | ✅ 点击 + Enter/Space | ⛔ 不适用 |
| **RadioButton** | ✅ 继承 ToggleButton + **GroupName**；**默认 Template** | ✅ 同 Toggle + GroupName + PointerRouter | ✅ **模板优先**（圆形 PART_Glyph） | ✅ 点击/Activate 组互斥 | ⛔ 不适用 |
| **TextBox** | ✅ Text/Placeholder/IsReadOnly/MaxLength/CaretIndex/CompositionText + **TextChanged（Signal）** + SelectionStart/Length（派生）；**默认 Template**（PART_Chrome 壳） | ✅ Text/CompositionText/Placeholder/CaretIndex/SelectionStart/SelectionLength/IsFocused/IsReadOnly/IsEnabled + ImeBridge/InputFocusRouter + **KeyboardRouter** | ✅ **PART_Chrome 壳** + 宿主文本/选区/caret 层 | ✅ TextChanged + IME + 键盘/剪贴板 | ⛔ 不适用 |
| **PasswordBox** | ✅ Password/PasswordChar/…；**默认 Template** | ✅ 镜像 **Text=掩码**；其余同 TextBox | ✅ 同 TextBox 模板壳 + 掩码文本层 | ✅ 同 TextBox；禁 Copy/Cut | ⛔ 不适用 |
| **Border** | ✅ Background/BorderBrush/BorderThickness/CornerRadius/Padding/Child | ✅ Background/BorderBrush/CornerRadius/BorderThicknessUniform + Padding + **ChromeHostHandle/ChromeRole（PART）** | ✅ 圆角底+描边；**PART_* 走宿主 VSM** | ⛔ 无交互（装饰容器） | ⛔ 固定少量子项例外 |
| **Slider** | ✅ Value/Minimum/Maximum/Step/Background/Foreground + **ValueChanged（Signal）**；**默认 Template**（PART_Chrome） | ✅ Value/Minimum/Maximum/Step/Background/Foreground/IsEnabled + IsMouseOver/IsPressed + PointerRouter（Slider 槽 · 拖拽/态色） | ✅ **模板优先**（PART Surface 轨+fill+thumb · VSM.Slider）；无模板宿主回退 | 🟡 ValueChanged + **鼠标拖拽/点击已接**；⛔ GUI 手测矩阵待补 | ⛔ 不适用 |
| **ProgressBar** | ✅ Value/Minimum/Maximum/IsIndeterminate/IsEnabled；**默认 Template**（PART_Chrome） | ✅ Value/Minimum/Maximum/IsIndeterminate/IsEnabled + BindPlatformMirror | ✅ **模板优先**（PART Surface 轨+填充 / IsIndeterminate 扫掠 · VSM.Progress）；无模板宿主回退 | ⛔ 只读反馈（无指针交互；Focusable=false） | ⛔ 不适用 |
| **ComboBox** | ✅ SelectedIndex/SelectedText（DP）+ ItemsSource（非泛型基座）/OptionCount/SelectedValue（`ComboBox<T>`）+ SelectionChanged；**默认 Template**（PART_Chrome 壳） | ✅ SelectedIndex/SelectedText/Font*/Background/Foreground/IsEnabled（`ComboBoxBase`） | ✅ **PART_Chrome 壳** + 宿主 SelectedText/chevron；展开态 Popup{ScrollView{ListView}}；无模板折叠 chrome 回退 | ✅ ARML/`ItemsSource` 非泛型轨；**`ComboBox<T>` SetOptions 泛型单态**：基类 DP 全名限定后 **arc-prune-001 已过**（ArmlDemo OnLoaded 冒烟） | ⛔ 不适用 |
| **Popup** | ✅ Child/PlacementX/PlacementY + Open/Open(Window?)/Close/IsOpen + **IsLightDismissEnabled** + **Closed/Opened（Signal&lt;bool&gt;）** + **ComputeInWindowPlacement** | ✅ 附加层根独立建树 + `RootEpoch` + 蒙层背景直写 + Layout 钳入 + Owner（显式 / Parent / MainWindow）+ **Open 末尾 ElementAddChild 置顶** | ✅ `PopupLayer`/`PopupBackdrop` 分支；多开靠窗口根 children 末尾序 | ✅ 蒙层点击（PointerRouter `PopupBackdrop` · 轻关闭门控）+ **Esc 严格 LIFO**（只裁决顶层；非轻关闭不穿透）+ **同窗口多弹层 Z 序（后开在上）**；同窗口互斥由消费方 `_activeCombo` 型锚点；✅ 滚动内翻；✅ ComboBox 钳高 ScrollView 外壳；⛔ 无蒙层非模态后置 | ⛔ 不适用 |
| **MessageBox** | ✅ **ShowAsync**(owner, text, caption, buttons[, image], ct) → `Task&lt;MessageBoxResult&gt;`；按钮 **OK / OKCancel / YesNo / YesNoCancel**；结果 OK/Cancel/Yes/No；**MessageBoxImage** None/Information/Warning/Error/Question | ✅ 经 **Popup** 附加层（`IsLightDismissEnabled=false`）；Surface + 正文主题色；OK/Yes=Primary / Cancel·No=Default；图标 = 主题色色块 + 符号 TextBlock（自绘几何） | ✅ 复用 PopupLayer/Backdrop + Button/TextBlock/StackPanel 通用递归（无专属 chrome） | ✅ 按钮点击完成 TCS；Esc→OKCancel/YesNoCancel=Cancel / YesNo=No / OK-only=OK（KeyboardRouter）；蒙层不关；禁 Win32/原生对话框；同窗单实例（再开先 Cancel 前者） | ⛔ 不适用 |
| **TabControl** | ✅ SelectedIndex + Children=`TabItem` + **内置页签栏 Header** | ✅ Background/SelectedIndex/TabCount/Header{i}/HeaderWidth{i}/HeaderBarHeight/**HeaderScrollOffset** + PointerRouter | ✅ 顶栏**按文案测宽左对齐** Header + **溢出 PushClip 水平滚** + 选中 Accent 底线 + LayoutShell 客户区（**未迁模板**：Panel 内容子树，`templated` 门禁不适用） | ✅ 页签栏点击切换 SelectedIndex（C HitTabIndex = localX+HeaderScrollOffset 累进）；✅ 顶栏滚轮调偏移 + 选中入视；⛔ 切换动画 / 关闭按钮 / 溢出箭头另排 | ⛔ 不适用 |
| **TabItem** | ✅ Header + Children 内容树 | ✅ Background/Header | ✅ **内容宿主**：Measure 有界时取视口宽高；Arrange 槽位=整页 `finalSize`（Stretch 拉满） | ⛔ 无独立交互（父容器互斥） | ⛔ 不适用 |
| **Image** | ✅ Source/Stretch/W/H/Background | ✅ Source/Stretch/Width/Height/Background（PlatformTreeSync.Image 分支 · `Window.Show()` 同步镜像树）+ TextureId 回写 | ✅ 解码纹理 `DrawTexture` 采样（GIF/SVG/静态；Stretch 与 VideoSurface 同源 UV；无纹理回退占位框） | ⛔ 显示效果 GUI 手测待补 | ⛔ 不适用 |
| **ScrollView** | ✅ Content/ScrollBarVisibility（H·V）/H·VOffset + ExtentWidth·Height/ViewportWidth·Height/ScrollableWidth·Height 只读 | ✅ 全属性 + ScrollRouter | ✅ 视口裁剪（scissor）+ 内容直通 + **竖滚动条**（`DrawVScrollBar` 轨道/滑块主题色；几何与 `rt_ui_vscroll_*` 同契约） | 🟡 滚轮/拖拽命中已解封；**Measure 交叉轴有界 + §4 预留条宽；Arrange 内容槽宽≥视口内容区**（修复窄列/条悬浮）；⛔ GUI 手测矩阵待补 | 🟡 内容须自带虚拟化（M-VZ1+） |
| **ListView** | ✅ 选择面五 DP + **SelectionChanged（Signal&lt;string&gt;）** + SelectionChangedHandler（string 事件名）均**继承 Primitives.Selector**（覆写 `OnSelectionApplied` 装箱 SelectedItem）；ItemsSource（ItemSourceView 强类型数据面）/ItemTemplate 继承 ItemsControl | ✅ SelectedIndex/LayoutHeight + 行 TextBlock ItemIndex（点击命中/高亮定位）+ PointerRouter（ListView 槽 · 点击路由）+ **Focusable/Tab 停靠** | ✅ LayoutShell 背景 + 行 TextBlock 递归（选中行高亮经行 TextBlock 呈现；无专属 wgpu 分支） | 🟡 点击选择已接（PointerRouter · 点击聚焦 + `SelectIndex`）；✅ **键盘导航**（Up/Down/Home/End/Enter → `Selector.TryHandleKey`）；✅ **SelectionChanged Signal.Subscribe 闭包链路（M-D0）**（`Signal&lt;string&gt;.Subscribe` 直挂 Action 表；headless `ui_selection_subscribe` + ArmlDemo `listSelHits`）；⛔ GUI 点击手测待补；⛔ Multiple 后置 | **必须**视口 + 回收池（M-VZ1+） |
| **ItemsControl** | ✅ ItemsSource（**唯一数据入口**：string/List/ObservableCollection/ItemSourceView 判别物化为强类型视图——object 本体 + 显示投影双通道；null 清空）/ItemTemplate/VerticalOffset/ItemHeight；（**ItemsPanel 已从公开面撤除**——未接线假能力禁留）；命令式 Set*Items 已撤面；DisplayMemberPath 已撤 | ⛔ 无分支 | ✅ LayoutShell 背景（泛化递归） | ⛔ 无（ItemsSource 物化 + 虚拟化非交互面） | **必须**视口 + 回收池（M-VZ1+） |
| **VirtualizingStackPanel** | ✅ VerticalOffset/ItemHeight/CacheLengthBefore/CacheLengthAfter/Orientation | ⛔ 无分支 | ✅ LayoutShell 背景（泛化递归） | ⛔ 无 | **视口窗口** · M-VZ1 · 回收池 |
| **Grid** | ✅ ColumnSpacing/RowSpacing/Background + ColumnDefinitions/RowDefinitions（List&lt;object&gt; 字段，非 DP）+ **Grid.Row/Column 附加属性（typed DependencyProperty&lt;int&gt;）** | ✅ ColumnSpacing/RowSpacing/Background | ✅ LayoutShell 背景（行列算法由 GridLayout 决定布局 rect，渲染只消费权威 rect） | ⛔ 无交互语义 | ⛔ 固定少量子项例外 |
| **DockPanel** | ✅ LastChildFill/Background + Dock 附加属性（string 占位，M3+ RegisterAttached） | ✅ LastChildFill/Background | ✅ LayoutShell 背景（停靠算法由 DockLayout 决定布局 rect） | ⛔ 无交互语义 | ⛔ 固定少量子项例外 |
| **WrapPanel** | ✅ Orientation/ItemWidth/ItemHeight/Background | ✅ Orientation/ItemWidth/ItemHeight/Background | ✅ LayoutShell 背景（换行算法由 Flexbox.ArrangeWrap 决定布局 rect） | ⛔ 无交互语义 | ⛔ 固定少量子项例外 |
| **Canvas** | 🟡 Left/Top/Right/Bottom 附加属性（string 占位，M3+ RegisterAttached）+ Background | ✅ Background | ✅ LayoutShell 背景（绝对定位由 CanvasLayout 决定布局 rect） | ⛔ 无交互语义 | ⛔ 固定少量子项例外 |
| **VisualHost** | ✅ Content/Child/Resources/GetHostResources/SetContent/Rebuild/Clear/Navigate + **ContentChanged/InnerLoaded/InnerUnloaded（Signal）** + IsDataContextBoundary | ✅ Background | ✅ 背景 + 内层根递归（内层根加载/卸载驱动重绘） | 🟡 ContentChanged/InnerLoaded/InnerUnloaded 通道；⛔ M-VH3 输入/焦点/HWND 后置 · **非 1GB 宿主** | 🟡 预览区推荐虚拟化模板 |
| **CodeEditor** | ✅ VerticalOffset/DocumentPath（DP 元数据；字段后备 __sinit 挂账）+ OpenPath/RenderVirtualizedLines/ContentExtentHeight/SetText + **IFrameDrawListProvider**（BuildFrameDrawList） | ✅ Font*/Background/Foreground + FrameDrawListRouter 登记（PlatformTreeSync.CodeEditor 分支） | ✅ 视口虚拟化 DrawList → `ExecuteDrawList(list, lx, ly)` + PushClip（`ElCodeEditor` 分支；局部坐标 + 布局原点；与 TreeDrawListBuilder 预览同构） | ⛔ IME/选区/LSP 后置 | **标杆** · CodeEditor 行视口 |
| **DataGrid** | ✅ SelectedIndex + **SelectionChanged（Signal&lt;string&gt;）** + SelectionChanged（ARML 事件名）**继承 Primitives.MultiSelector**；**ItemsSource 唯一行入口**（`List&lt;List&lt;string&gt;&gt;` / `ObservableCollection&lt;List&lt;string&gt;&gt;` 多列；`List&lt;string&gt;` / `ObservableCollection&lt;string&gt;` 单列；可观察源 **CollectionChanged 增量改行**；禁 AddRow 字符串重载双轨）+ AddColumn/GetCell/ClearRows/SelectIndex + RowHeight/HeaderHeight/VerticalOffset | ✅ SelectedIndex/ColumnCount/Header{i}/Width{i}/RowHeight/HeaderHeight/RowCount + 行镜像 ItemIndex/C{i}/Layout*（PlatformTreeSync DataGrid/DataGridRow 分支）+ PointerRouter（DataGrid 槽 · HitItemIndex/HitMods） | ✅ 专属分支 `RenderDataGrid`（**未迁模板**：`ControlTemplate.ApplyTo` 清子树会毁行镜像；表头/斑马/选中过大，诚实后置） | 🟡 点击选择已接（C `rt_ui_datagrid_hit_row` + HitMods → `SelectIndexWithMods`）；✅ **SelectionChanged Signal.Subscribe 闭包链路（M-D0）**（单选 `SelectIndex` + 程序化多选 + **Ctrl/Shift 修饰键手势**；headless `ui_selection_subscribe` + `ui_datagrid_multi_select` + ArmlDemo `gridSelHits`/`multiSel`/`modsSel`）；⛔ GUI 手测待补；⛔ 列拖拽排序/编辑后置；**Observable 多实例并发订阅 ✅**（route 槽 + int 按值捕获；ItemsControl/ItemSourceView 同款多槽 ✅） | **必须**行虚拟化 · M-VZ4 ✅（视口窗口 + 回收池；Extent=rowCount×stride 纯算术；e2e `ui_datagrid_selection_e2e` + C `ui_datagrid_selection_c_e2e`） |
| **TreeView** | ✅ Header/IsExpanded/IsSelected (TreeViewItem) + SelectedIndex/SelectedItem/SelectionChanged (tree root; **not** Selector) + **Focusable/Tab** + **ItemsSource (`List&lt;TreeNode&gt;`) + VerticalOffset/ContentExtentHeight/EnsureViewportMaterialization** | ✅ TreeView/TreeViewItem + PointerRouter + FocusManager + VerticalOffset/ExtentHeight/VisibleRowCount mirror | ✅ LayoutShell clip + TreeViewItem header chrome (wgpu only) | ✅ expand/select + keyboard + **FlatIndex viewport smoke** (ArmlDemo Bulk>=80) | 🟡 **M-VZ4 min OK: shallow FlatIndex window + recycle pool**; ⛔ **not** full hierarchical path virt |

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
| `Selector` | `std/UI/Core/Components/Primitives/Selector.as` | 单选语义层（WPF Selector 对标；Control → ItemsControl → **Primitives.Selector** → ListView / ComboBoxBase → ComboBox&lt;T&gt;）：SelectedIndex/SelectedItem/SelectedValue/SelectedValuePath 四 DP + `SelectIndex` 模板方法选中流程（校验 → 写点 → 镜像同步 → 附加同步 → 通知）+ 五 protected virtual 钩子（`SelectionItemCount`/`ApplySelectedIndexCore`/`OnSelectionApplied`/`SelectionPayload`/`RaiseSelectionChanged`）+ SelectionChanged Signal&lt;string&gt;；DataGrid 经 protected ctor `ownsItemsHost=false` 自管行宿主（新**扁平**单选集合类控件一律派生此层复用选择面，禁止重写选中流程）。**TreeView 不派生本层**（层级节点选中 + 展开折叠与扁平索引模板不同构；自管 DFS FlatIndex + 自有 `TryHandleKey`） |
| `MultiSelector` | `std/UI/Core/Components/Primitives/MultiSelector.as` | 多选语义层（WPF MultiSelector 对标；**Primitives.MultiSelector** → DataGrid）：派生 `Primitives.Selector` 承接全单选面 + SelectionMode/SelectedItems/SelectItem/SelectAll/ClearSelection/`SelectIndexWithMods`；`SelectIndex` 经 `SyncSelectionCollection` 替换 SelectedItems；**程序化多选 ✅**；**Ctrl/Shift 修饰键手势最小面 ✅**（Multiple：Ctrl 切换 / Shift 锚点范围 / 无修饰替换；headless `ui_datagrid_multi_select`）；镜像仍单值 SelectedIndex |
| `InputElement` | `std/UI/Markup/InputElement.as` | 输入组件公共基类（WPF 同构）：焦点管理 + 键盘路由 + 默认激活（Activate）；Button/TextBox/ContentControl 继承 |
| `TextBoxModel` / `TextBoxController` | `std/UI/Editing/TextBoxModel.as` · `std/UI/Core/Internal/TextBoxController.as` | TextBox 编辑内核（纯逻辑 · 撤销/重做/选区 · 无头可测）+ 平台事件→模型操作路由（internal，不暴露开发者） |
| `ContentPresenter` | `std/UI/Components/ContentPresenter.as` | ControlTemplate 内 Content 呈现占位（OnLoaded 沿父链同步 ContentControl.Content；ApplyTo 显式注入） |
| `UserControl` / `Page` | `std/UI/Components/UserControl.as` · `Page.as` | ContentControl 语义子类（UserControl 空类；Page 有 Title DP） |
| `Application` | `std/UI/Components/Application.as` | MainWindow/Resources + Run·RunAsync/RunCore/OnStartup/OnExit（隐式样式 + IME handler 装配） |
| `ICommand` | `std/UI/Components/ICommand.as` | CanExecute/Execute 接口（Button.Command 预留 · MVVM） |
| `ItemContainerGenerator` | `std/UI/Components/ItemContainerGenerator.as` | ItemsHost 回收池 + 视口物化（M-VZ1；ItemsControl 内部接线，非控件） |
| `RowDefinition` / `ColumnDefinition` | `std/UI/Components/Layout/RowDefinition.as` · `ColumnDefinition.as` | Grid 行/列定义（GridLength.Auto/Star/px） |
| `WindowHost` | `std/UI/Components/WindowHost.as` | 静态 ABI 桥（ElementCreate/Set*/Get*/AddChild/IME/键盘/滚动 handler）；`NativeHandle`/`CreateWindow`/`RunEventLoop` 为互操作面 |

## 默认观感（Light Theme · Ant Design 6.x 令牌）

> **双对齐（唯一）**：交互语义 / API 对标 WPF；**默认皮肤**对标 [Ant Design 6 Design Token](https://ant.design/docs/react/customize-theme)（`defaultAlgorithm` Light；Dark 为 `darkAlgorithm` 输出快照）。**令牌对齐，不宣称像素级 DOM/CSS 复刻**。色值权威源 `Themes/Light.arml` / `Dark.arml`。架构见 [theme-style-interaction-architecture](../../../docs/rfc/037-ui/references/theme-style-interaction-architecture.md)。
>
> **核对（2026-09-08）**：下列值与 `Light.arml` + `BuiltInTheme.Colors.g.as` + `rt_ui_design_tokens.h` 一致。

| Token | Ant 源 | 值 | 消费方 |
|-------|--------|-----|--------|
| `Color.Primary` | colorPrimary | `#1677FF` | Button 默认填充 |
| `Color.Primary.Hover` | colorPrimaryHover | `#4096FF` | Button Hover（VSM 覆盖静态 Background） |
| `Color.Primary.Pressed` | colorPrimaryActive | `#0958D9` | Button Pressed |
| `Color.Background` | colorBgLayout | `#F5F5F5` | Window / 页底 |
| `Color.Surface` | colorBgContainer | `#FFFFFF` | TextBox / 面板 |
| `Color.Border` | colorBorder | `#D9D9D9` | TextBox 描边 |
| `Color.Border.Disabled` | colorBorderDisabled | `#D9D9D9` | Disabled 描边（6.x Map） |
| `Color.Text.Primary` | colorText | `rgba(0,0,0,0.88)` | 正文 |
| `Color.Danger` | colorError | `#FF4D4F` | 错误 / 危险 |
| `Color.Success` | colorSuccess | `#52C41A` | 成功（键已注册） |
| `Color.Warning` | colorWarning | `#FAAD14` | 警告（键已注册） |
| `Color.Focus.Ring` | colorPrimary @ 40% | `#1677FF` @ 40% | `:focus-visible` |
| `Radius.Control` | borderRadius | `6` | ControlMetrics / VSM |
| `controlHeight` | controlHeight | `32` | ControlMetrics.ControlHeight / InputMetrics.MinHeight |
| `Color.Danger.Hover` | colorErrorHover | `#FF7875` | Danger Button Hover |
| `Color.Danger.Pressed` | colorErrorActive | `#D9363E` | Danger Button Pressed |
| `Motion.Duration.Fast` | motionDurationFast 近似 | `120ms` | hover / press 默认 |
| `Motion.Duration.Normal` | motionDurationMid 近似 | `160ms` | focus 默认 |
| `Motion.Easing.Standard` | motionEaseOut 近似 | `ease-out` | MotionEngine 默认曲线 |
| `Motion.Easing.Linear` | — | `linear` | 备选曲线键 |
| `Motion.Easing.In` | — | `ease-in` | 备选曲线键 |
| `Motion.Easing.InOut` | — | `ease-in-out` | 备选曲线键 |

**诚实缺口（本刀）**：Image/CodeEditor wgpu 路径已闭合但 **完整 GUI 手测矩阵待补**；**独立 ScrollBar 控件延后**（竖条嵌于 ScrollView；C `rt_ui_vscroll_*` 句柄绑定；抽控件须单独立项）；**Dark 未跑完整 darkAlgorithm 运行时**；**每控件/每态曲线覆写**后置（全局 `Motion.Easing.Standard` 已立并接线；ProgressBar `IsIndeterminate` 已接 `ResolveLoop01`）；**空间脏矩形 Present 已挂起**（swapchain 不可 `LoadOp_Load`；`InvalidateRegion` 升整窗 Clear，待离屏保留缓冲后再启）；非控件级失效树后置。Button dashed/text/link + Size 已由 keyed Style 多绑定立（见 `Themes/Controls/Button.arml`）。**`IsFocusVisible`**：键盘 Tab/方向 → 焦点环；指针聚焦 → 清环（caret 仍跟 `IsFocused`）。

> **核对增补（2026-09-09 · swapchain 禁区域 Load）**：根因——Fifo 每帧新 surface texture，`LoadOp_Load`+根 scissor 只重画脏区 → 其余像素为过期/黑（ArmlDemo 启动黑屏闪烁）。处置：`InvalidateRegion`→整窗 `Invalidate`；`HasPresentRegion` 恒 false。保留 API；离屏保留缓冲就绪后再恢复区域 Present。

> **核对增补（2026-09-08 · 空间脏矩形 Present）**：~~`FramePump.InvalidateRegion` 并集 DIP 脏区；`WgpuRender.BeginFrame` 区域帧 `clear=0`（Load）+ 根 `PushClip`~~ → **已挂起**（见上条 2026-09-09）。caret 翻转仍标脏但升整窗；`ForceFullPaint` 于 layout/Motion/surface 重配。禁 LINQ 关键字作局部名（`by`）。列拖拽排序/编辑仍后置（面过大）。

> **核对增补（2026-09-08 · Motion 曲线 token）**：`BuiltInTheme` 增 `Motion.Easing.{Standard,Linear,In,InOut}` 字符串资源；`MotionEngine.EaseProgress` 读 `Standard` 选 `Ease*` 族；禁 Class/SizeMode/Variant。

> **核对增补（2026-09-08 · 绘脏/布局脏分区）**：`FramePump.Invalidate` = 纯绘（整窗）；`InvalidateLayout` = Measure/Arrange + 整窗绘；`InvalidateRegion` = **升整窗**（swapchain 约束）；`SetValue`/`AddChild`/客户区尺寸/Text 绑定走布局脏；caret 翻转标脏（半秒不再全树 Relayout）。

> **核对增补（2026-09-09 · Style 短键标准 §0.1.1）**：① 作者**只写** `Primary, Small`；查找 = `{TargetType}.{K}` → 全局 `K`（`Shared.arml`）；② Shared 尺寸成套（MinHeight+Padding）；隐式 `BasedOn Medium`；**仅差异**才写 `{T}.Small`；**禁** `Button.Size.SM` 双轨与无差异空覆写；③ **删 Appearance DP**；④ ArmlDemo 短键；⑤ 门禁断言回退顺序与对齐表。

> **核对增补（2026-09-09 · Style 单一惯用 + 每控件一文件）**：① VSM = Style 内部交互态引擎；② Padding → `Size.Control.Padding*`；③ **合并** `*.Styles.arml` 进各控件主 `*.arml`。

> **核对增补（2026-09-09 · 硬编码样式清扫）**：① Controls `*.arml` 禁裸 Thickness / 裸 `#hex`（门禁 `controls_arml_no_bare_thickness_or_hex`）；Border → `Size.Border.Thickness`/`Padding`；② 补键 `Color.Text.Selection` / `Color.Image.Fill|Border` / `Color.Text.Highlight`；③ RenderTree/TreeDrawList/MessageBox/Popup/ComboBox/ArmlDemo 主题色一律 `ResolveColor` / `{StaticResource Color.*}`；④ 删死码 `ColorBorder()` Fluent 灰。

> **核对增补（2026-09-09 · 主题资源键命名收敛）**：统一 `Color.*` / `Size.Control.Height*`；`Negative`→`Danger`（与 Style 短键成套）；删 `Accent.Gradient.A/B`（改用 `Color.Primary`）；`Placeholder`→`Color.Text.Placeholder`；`Image.Placeholder.*`→`Color.Image.*`；`Scroll.Thumb.Active`→`.Pressed`；删 `Size.Control.PaddingX/Y` 冗余键。规范表见 [builtin-theme-resources](../../../docs/rfc/037-ui/references/builtin-theme-resources.md)。

> **核对增补（2026-09-08 · Ant Design 6 令牌 + 交互 P0）**：口径升 6.x；`colorBorderDisabled`；Button Hover/Pressed 覆盖宿主静态 Background；`x:Bind` SyncText 触发 Invalidate；PointerRouter Dictionary 破槽上限。Accent 渐变仍为实色 Primary（禁紫蓝混搭）。

> **核对增补（2026-09-08 · Ant Design 默认皮肤）**：BuiltInTheme / Light·Dark.arml / C 头 / InputMetrics 高度对齐 Ant token；ArmlDemo 硬编码 Fluent 色清掉。

> **核对增补（2026-09-08 · 基础控件完整度）**：① `ProgressBar` 三层闭合（DP + RenderTree/VSM.Progress + PlatformTreeSync；只读）；② `RadioButton`（GroupName 互斥 + 圆形 chrome + PointerRouter）；③ ComboBox ARML/`ItemsSource` 非泛型轨封口（codegen→`ComboBoxBase`；基座 `ApplySelectedIndex`/`SelectedText`）；④ Slider hover/pressed 态色接线；⑤ ArmlDemo Controls 页回挂 Progress/Radio/ComboBox。

> **核对增补（2026-09-08 · Border）**：① `Border : Control`（Focusable=false）Background/BorderBrush/BorderThickness/CornerRadius/Padding/Child；② Measure/Arrange 扣描边+内边距；③ wgpu 圆角底+描边；④ 隐式 Style → Color.Surface/Border + Size.Border.Thickness/Padding + Radius.Control；⑤ ArmlDemo Controls 页嵌套演示。非均匀描边绘制取 max 均匀宽（本切片）。

> **核对增补（2026-09-08 · TextBox 剪贴板）**：① `rt_ui_clipboard_get_text`/`set_text`（Win32 CF_UNICODETEXT；非 Win32 stub）；② `TextBoxController` Ctrl+C/V/X；单行 Paste 剥 CR/LF；③ PasswordBox `AllowsClipboardCopy=false`——禁 Copy/Cut 明文出剪贴板，允许 Paste；④ 文档从 text-editing 非目标迁出。

> **核对增补（2026-09-09 · TextBox 双击词选 · ListView 键盘 · ComboBox 钳高）**：① `ImeBridge.RouteInputClick` 500ms/4DIP 双击 → `SelectWordAt`（空白分词 · UTF-8）；三击行选后置；② ListView Focusable+Tab 停靠 + `Selector.TryHandleKey`（↑↓/Home/End/Enter）；点击聚焦；③ ComboBox `Popup{ScrollView{ListView}}` 钳高可滚。

> **核对增补（2026-09-09 · TreeView ItemsSource 层级最小面）**：`TreeNode`（Header/Children/IsExpanded）+ `TreeView.ItemsSource=List<TreeNode>` 递归物化 TreeViewItem（Tag=节点）；禁 DisplayMemberPath；**未**开 Observable 层级（语言缺口诚实）；ArmlDemo Tree 改绑；M-VZ4 FlatIndex min: see note below. 

> **核对增补（2026-09-09 · TreeView 键盘导航最小面）**：TreeView 改 `Control` 基类（仍不派生 Selector）+ Focusable/Tab + `TryHandleKey`（可见扁平行 ↑↓/Home/End；←→ 折叠/展开；Enter）；FocusManager/PlatformTreeSync/PointerRouter 接线；ArmlDemo Tree 页冒烟；M-VZ4 FlatIndex min: see note below. 

> **核对增补（2026-09-09 · TreeView M-VZ4 FlatIndex viewport min）**：ItemsSource path uses visible FlatIndex `ItemViewport` window + `_rowPool`; `VerticalOffset`/`ContentExtentHeight`; expand rebuilds FlatIndex; ArmlDemo Bulk>=80; honest: shallow FlatIndex virt, NOT full path virt.


> **核对增补（2026-09-08 · PasswordBox）**：① `PasswordBox : TextBox` 复用 TextBoxModel/Controller/ImeBridge；② 镜像 Text=掩码（PasswordChar 默认 ●）、Composition 恒空；③ GeometryText 命中/Measure 同源；④ Style `PasswordBox` + `PasswordBox.Size.*`；⑤ C 指针/IBeam 同族；⑥ **复制策略**：禁 Copy/Cut；允许 Paste（见上条剪贴板增补）。

> **核对增补（2026-09-08 · ProgressBar.IsIndeterminate）**：RenderTree 扫掠段 + `MotionEngine.ResolveLoop01`/`CancelLoop`；周期 = `Motion.Duration.Normal` × `ControlMetrics.ProgressBarIndeterminatePeriodFactor`；段宽 `ProgressBarIndeterminateFraction`；FramePump 经 `Active()` 含循环槽保帧；ArmlDemo Controls 页不定长条。

> **核对增补（2026-09-09 · Tab 溢出滚动最小面）**：① 镜像 `HeaderScrollOffset`；② RenderTree 顶栏 `PushClip` + `cursorX=lx-offset`；③ C `HitTabIndex` 用 `localX+offset` 与渲染同源；④ 顶栏滚轮（`scroll_win32` 认 TabControl 栏 + ScrollRouter→`ApplyHeaderWheelDelta`）；⑤ 选中切换 `EnsureSelectedTabVisible`；⑥ ArmlDemo 追加 10–12 长 Header 挤栏冒烟。⛔ 切换动画 / 关闭按钮 / 溢出箭头另排。

> **核对增补（2026-09-09 · DataGrid Ctrl/Shift 多选手势最小面）**：① `MultiSelector.SelectIndexWithMods`（mods bit0=Shift bit1=Ctrl）；Multiple：Ctrl 切换 / Shift 锚点范围 / 无修饰替换（同 `SyncSelectionCollection`）；② C `HitMods` + PointerRouter DataGrid 槽直调；③ 选中成员以逻辑下标表为准；④ headless `datagrid_mods_*` + ArmlDemo `modsSel`；⛔ GUI 真键手测后置；镜像仍单值 SelectedIndex。

> **核对增补（2026-09-09 · Tab 测宽左对齐 + 点击）**：① 页签 Header **内容测宽左对齐**（`HeaderWidth{i}` = 文案测宽 + 2×`TabHeaderPaddingX`；禁均分拉满栏宽）；② C `HitTabIndex` 按 HeaderWidth 累进命中；③ `RT_UI_CONTROL_HANDLER_MAX` 8→32——原 8 槽在 DataGrid 后丢弃 TabControl/ComboBox/PopupBackdrop 注册，导致页签栏点击无回调。

> **核对增补（2026-09-08 · Tab/Scroll 宿主拉满）**：① `TabItem` Arrange 槽位=整页 `finalSize`（禁 DesiredSize 槽）；② `TabControl`/`TabItem` Measure 有界时取视口；③ `ScrollView` 竖滚交叉轴有界测布 + §4 预留条宽 + Arrange 内容槽宽≥视口；④ ArmlDemo 消费方页签选中 chrome。

> **核对增补（2026-09-08 · 可见性 / 继承完备）**：① `Control.Foreground` DP 默认近黑（Light Text.Primary 冷启动保底）；② **环境值镜像**：未本地/样式/继承的 Foreground / 未设 Background 不上平台镜像，渲染与 `StateColor` 回落活动主题键（Text.Primary / VSM），`SwitchTheme` 未显式着色的控件跟随；③ `TabControl`/`TabItem` 最小切片 + ArmlDemo 页签切换；④ codegen `SelectedIndex` 等整型属性发数字字面量；⑤ ComboBox 下拉底色走 `Color.Surface`。

> **核对增补（2026-09-09 · Popup 多弹层 Z 序）**：① `rt_ui_element_add_child` 同父已挂载 → 移至 children 末尾；② `Popup.Open` 每次 ElementAddChild 置顶（复开亦然）；③ Esc `TryDismissTopOnEscape` 严格 LIFO（顶层非轻关闭不穿透）；④ ArmlDemo「Open stacked Popup」冒烟。后置：无蒙层非模态。

> **核对增补（2026-09-08 · Popup M1）**：① PointerRouter 接线 `PopupBackdrop` + `ComboBox` 点击槽；② `IsLightDismissEnabled`（蒙层/Esc 门控；MessageBox 前置挂钩置 false）；③ `Open(Window?)` Owner（Parent → MainWindow 回退）；④ Esc 经 KeyboardRouter（平台撤无条件 Esc 退窗）；⑤ ComboBox `Open(owner)`。多弹层 Z 序 ✅（见上条）；钳高 ScrollView 已由 ComboBox 消费方闭合。

> **核对增补（2026-09-08 · DataGrid Observable 行增量）**：`ItemsSource` 接 `ObservableCollection&lt;List&lt;string&gt;&gt;`（多列）/ `ObservableCollection&lt;string&gt;`（单列）；`CollectionChanged` 增量改扁平 `_cells` + `RefreshWindow`（禁每次全量 Clear+重灌）；ArmlDemo BooksGrid 改可观察并 Add 冒烟。

> **核对增补（2026-09-08 · DataGrid Observable 多实例）**：破单活跃槽——`_obsHosts` 槽表 + `OnChanged` 仅按值捕获 route 槽 int（BindingOperations 同款）；ArmlDemo 第二幽灵网格并发 Add 冒烟 `peer=`。
>
> **核对增补（2026-09-08 · ItemsControl / ItemSourceView 多实例）**：同 DataGrid——`_viewHosts` / `_obsViews` + route int 按值捕获；ArmlDemo ListView peer `extent0/extent1` / `peer0/peer1` 冒烟（禁再 OnLoaded 掩盖）。几何余量→ControlMetrics（ProgressBar/Slider/MessageBox/RenderTree 焦点环·滚动条·Combo/Tab）；门禁 `items_control_observable_multi_route` + `control_metrics_owns_geometry`。
>
> **核对增补（2026-09-08 · Chevron/Tab 微几何 + ScrollBar 裁决）**：① Combo chevron 堆叠/Tab 栏高·字号·指示条 → `ControlMetrics`；② VSM `ScrollBar` 圆角 → `VScrollThumbRadius`；③ MessageBox 图标徽章 → `MessageBoxIconSize`；④ **独立 ScrollBar 延后**（C `rt_ui_vscroll_*`↔ScrollView；嵌套竖条为 §4 能力面）。

> **核对增补（2026-09-08 · DataGrid ItemsSource + ItemsPanel 撤面 + ComboBox&lt;T&gt;）**：① DataGrid 行数据唯一入口 `ItemsSource`（`List<List<string>>` 多列 / `List<string>` 单列）；撤 `AddRow(string…)` 字符串重载双轨；② `ItemsPanel` 未接线假能力从 ItemsControl/ListView 公开面与 arc-ui typeck 移除；③ RenderTree DataGrid 行高/内缩改 `ControlMetrics`；④ `ComboBox<T>.SetOptions` 基类 DP 全名限定消 arc-prune-001；⑤ ArmlDemo Data/Controls 回挂。

> **核对增补（2026-09-08 · Popup 内翻）**：① `Popup.ComputeInWindowPlacement` 窗口内翻定位（优先下方、溢出翻上方、两侧不足钳高）；② ComboBox `OpenDropDown` 接入；③ `LayoutPopupContent` 客户区兜底钳入；**下拉主题化 ✅**；**钳高 ScrollView 外壳 ✅**（`Popup{ScrollView{ListView}}`）。

> **核对增补（2026-09-08 · IN-R2）**：① 平台 `keyboard_win32` 机械转换 key/text，禁 `IsReadOnly`/Shift 扩选分支；② `TextBoxController.HandleKey` 承接 Ctrl+A/Z/Y、方向/词粒度、Home/End/Delete/Backspace/Space；③ `ImeBridge.OnNativeEvent` 仅 composition/commit/focus_lost。
>
> **核对增补（2026-09-07 · Input）**：① TextBox 渲染/`HandleClick`/`HandleDrag` 统一 `InputMetrics.PenOriginX`（Wgpu `MinTextPaddingX` 对齐 LayoutHelper=8）；② FocusManager 启动优先首个 TextBox，`ActivateDefaultFocus` 不再旁路抢焦点；③ 鼠标拖选经 `SetControlDragHandler("TextBox")` + 局部 DIP X。
>
> **核对增补（2026-09-07 · 下一刀）**：① FocusManager/InputFocusRouter 自固定 8 槽静默丢弃 → `List` 动态表（RFC 037 §8）；② RenderTree Button/TextBox/ComboBox 行高改 `EstTextHeight` 与 DrawText 同源；③ Toggle/CheckBox 键盘 Activate 与 TextBox 选区绘制口径翻新为已接。

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
| `rt_ui_set_button_click_handler` / `rt_ui_set_key_handler` / `rt_ui_set_text_handler` / `rt_ui_set_scroll_wheel_handler` | platform `window.cpp` / `keyboard_win32.c` / `scroll_win32.c` | Button Click · IN-R2 单一键盘通道 · ScrollView 滚轮 |
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
| RadioButton | `Components/RadioButton.as` | GroupName 互斥 + 圆形 chrome |
| ProgressBar | `Components/ProgressBar.as` | Value/Min/Max 比例填充；IsIndeterminate → MotionEngine 扫掠 |
| PasswordBox | `Components/PasswordBox.as` | 掩码显示 + Password API；Paste 允许；禁 Copy/Cut |
| Border | `Components/Border.as` | Surface/Border/Radius.Control 隐式 Style；Child 布局装饰 |
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
| TreeView | `Components/TreeView.as` · `TreeViewItem.as` · `TreeNode.as` | **M-VZ4 min**: ItemsSource FlatIndex window + recycle pool + Extent; honest: not full hierarchical path virt |

## 演示

- `examples/ArmlDemo` — 单一综合演示：TabControl 12 页签（… · DataGrid · TreeView ItemsSource · **溢出长 Header 10–12**）；`arc build examples/ArmlDemo`

## 验证

```text
cargo test -p arc-ui
```

> 注：原 `ui_skeleton_honesty_e2e` 已随 `arc-integration` 退场（a2627a0f），
> 骨架证据面由 `crates/arc-ui/tests/` 承接。

渲染快照验收（M2+）：wgpu 默认 Theme，无用户 Style 覆盖。
