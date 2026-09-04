# Arc.UI 生产面契约（字体 · 对齐 · 分层控件 · 滚动条）

> 本文是 [037 UI 声明式框架(../../037-ui.md) 的**渐进式披露子项**。定义「生产级」内置面的**分层完备性**与滚动条行为契约。**未经验收协议不得宣称生产完备**（宣称纪律）。
>
> 主题 ARML 正道见 [builtin-theme-resources](builtin-theme-resources.md)；字体最小面见 [custom-fonts](custom-fonts.md)。

## 1. 分层派生：每层能力必须闭合

WPF 同构层级（Arc 已立）：

```
Element → FrameworkElement → Control → ContentControl → Button / CheckBox / Window / …
                          ↘ Control → InputElement → TextBox / Slider / Image / … · Control → TextBlock
FrameworkElement → Panel → StackPanel / Grid / DockPanel / WrapPanel / Canvas / ScrollView
```

| 层 | 必须完备的能力 | 禁止半成品 |
|----|----------------|------------|
| **Element** | DP / Signal、逻辑树、`TypeName`、平台句柄镜像字段 | 仅解析无布局 |
| **FrameworkElement** | Measure/Arrange、`Margin`、`Width/Height`、`HorizontalAlignment`/`VerticalAlignment`、`Resources` | DP 存在但布局忽略 |
| **Control** | `Background`/`Foreground`、字体三件套、`IsEnabled`、`Focusable`/`IsTabStop`、VSM 可查询状态 | 可交互控件未设 Focusable |
| **ContentControl** | `Content` + **ContentAlignment** 影响内容槽布局与绘制 | ContentAlignment 仅解析不生效 |
| **Panel / ScrollView** | 子代 Arrange 尊重对齐；ScrollView 视口裁剪命中与绘制一致 | 滚动条仅绘制无输入或仅输入无样式 |

**内置 chrome 正道**：`VisualStateManager` 状态色 + `WgpuRender.RenderTree` 分支。模板体系（WPF 对齐）已立：`Setter Property="Template"`（`Setter.TemplateValue` 载荷字段）走 StyleEvaluator **通用 DP 路径**——属性集合由源组件 DP 注册表决定（`ResolveProperty` 动态解析，评估器零属性名感知），ControlTemplate 经 wrapper 通道写入 Template DP 并套用 `ApplyTo`（换树幂等）；**Style.Triggers** 属性触发器（`StyleManager.EvaluateTriggers` 进入/退出生命周期——条件命中应用触发 Setters、失效快照回退，string/bool/int 载荷条件，BasedOn 链叠加）；**隐式 DataTemplate**（`ResourceDictionary.AddTemplate/LookupTemplate` 按 DataType 匹配 + `ContentControl.DataContent/DataTypeName` 数据载荷 + `ContentPresenter.ApplyDataTemplate` 显式模板优先 → 隐式匹配 → 兜底文本）；**TemplateBinding**（`TemplateBindingPropertyKey` 附加属性标记，`ApplyTo` 建立绑定时经 `Element.Observe`→`Signal.OnChanged` 静态方法组订阅宿主 DP 变更——宿主属性变更自动重同步模板树，单活跃宿主全自动、多宿主经 `RefreshBindings(host)` 手动兜底）；**模板多实例工厂化**（`ControlTemplate.Instantiate` 委托，多宿主独立视觉树）。`WgpuRender.RenderTree` 内置 chrome 分支（Button/CheckBox/TextBox/Slider）**模板让位**——元素已挂视觉子树（`templated`）时跳过内置 chrome、仅走通用递归，杜绝 chrome + 模板双轨叠加；`TreeDrawListBuilder` 设计时预览以**同构门禁**（已挂子树跳过文本 chrome）与之对齐。ItemsControl 类容器例外（ComboBox）：恒有子节点，其 chrome 分支以提前 `return` 跳过通用递归（折叠态只画 chrome、不铺子项）。新增/重构组件的三层编写动作见 §6 checklist。

**Trigger/VSM 分工边界（双轨禁令）**：交互态视觉（hover/pressed/focus/disabled/checked/selected）唯一正道是 `VisualStateManager` internal 强类型配方（RFC 037 §1 态反馈唯一来源）——**禁止**用 Style.Triggers 表达交互态；Style.Triggers 仅管通用属性条件样式（如 Content=="特定值" 换色）。

**资源引用单一惯用法**：`StaticResource` 唯一——**不设 DynamicResource**。主题切换的动态性由 VSM 三层解析契约承担（渲染器经资源键动态解析，切主题即全链生效，用户 arml 覆盖经 MergedDictionaries 本地优先），开发者语法面保持一种写法（RFC 037 §1 单一惯用法原则）。

## 2. 字体体系（生产完备）

命名族 + Normal/Bold + 同源度量（custom-fonts 最小面）为底。生产完备面另要求：

| # | 要求 |
|---|------|
| F1 | `Control` 字体三件套经隐式 Style / 继承，在 TextBlock/Button/TextBox/CheckBox 上布局与绘制同源 |
| F2 | atlas 文本采样与覆盖 AA 达到可读生产质感（Linear + 覆盖抗锯齿） |
| F3 | 未注册族名回退默认族且不记假成功（见 custom-fonts） |

**非目标（仍守 custom-fonts §6）**：pack URI、HarfBuzz、彩色 emoji、`FontStyle`——**不得**借「健全」偷渡；若要开，须先改该 RFC。

## 3. 对齐体系

| 面 | 契约 |
|----|------|
| **子元素对齐** | `HorizontalAlignment` / `VerticalAlignment` 经 `LayoutHelper.ArrangeChild`；Stack/Grid/Dock/Wrap/Canvas/ScrollView **同一语义** |
| **内容对齐** | `HorizontalContentAlignment` / `VerticalContentAlignment` 仅作用于 ContentControl 内容槽（含 Button 文案）；须同时驱动 Measure/Arrange 与 RenderTree 文本位置 |
| **Canvas** | 绝对定位优先；Stretch 与绝对槽冲突时文档化并测一格，禁止静默随机 |

## 4. 滚动条生产契约（竖条为能力面；横条另立门禁）

对标 WPF `ScrollBarVisibility`：

| 值 | 绘制 | 输入 | 视口 |
|----|------|------|------|
| `Auto` | 仅 `extent > viewport` 时显示 | 显示时才可拖/点 | 显示时预留条宽 |
| `Visible` | 总是显示轨道（可空滑块） | 可交互 | 总是预留条宽 |
| `Hidden` | 不绘制 | 不命中 | 不预留 |
| `Disabled` | 不绘制（或灰态占位，二选一须单一） | **不滚动**（滚轮/拖/程序 SetOffset 均钳制） | 不预留 |

| 交互 | 契约 |
|------|------|
| 滑块拖动 | 连续 `SET_OFFSET` |
| 轨道空白点击 | **按页** `PAGE_UP` / `PAGE_DOWN`（一页 ≈ viewport）；禁止与「跳转到点击比例」双轨并存 |
| 滚轮 | 作用于命中的最内层 ScrollView；偏移 DIP |
| 几何 | C `rt_ui_vscroll_*` 与 `DrawVScrollBar` **同一公式**（宽、最小滑块、travel） |
| 样式 | 轨道/滑块色经主题 `Color.Scroll.Thumb(+Hover)` + VSM `ScrollBar`；禁止硬编码灰 |

**非目标（能力边界）**：横向滚动条 UI、overlay 自动隐藏动画、>8 ScrollView 槽扩容（后续能力，须诚实登记）。

## 5. 焦点（生产阻断项）

| 项 | 契约 |
|----|------|
| 可交互内置控件 | Button / Input / CheckBox / ToggleButton / Slider 等须正确 `Focusable`+`IsTabStop`（或显式 false 并文档化） |
| Tab 容量 | ≤8 槽；超出容量的生产级动态扩容属后续能力（须先立契约） |
| Input | 点击命中 → `FocusManager` + `ImeBridge` + 平台 `IsFocused` + caret 同帧 |

## 6. 内置组件三层编写契约（checklist）

新增或重构任一内置控件（Button/Slider/ComboBox 一类），**三层必须同构闭合**——缺任一层即为 §1 所禁半成品。此为本仓库内置组件的标准编写法（单一惯用法，禁止各组件自创接入方式）：

| 层 | 落点 | 契约 |
|----|------|------|
| ① 控件类 | `std/UI/Core/Controls/*.as` | 视觉相关属性一律 DP 化（供 VSM / TemplateBinding / 平台同步消费，禁止裸字段，如 ComboBox `SelectedText`）；布局契约闭合——Measure 响应内容、`ArrangeOverride` 尊重内容对齐（如 ComboBox 单行测量） |
| ② 运行时渲染 | `WgpuRender.RenderTree.as` | 有内置 chrome 的控件须有类型分支；chrome 绘制前**模板让位**（已挂视觉子树 `templated` → 跳过 chrome、走通用递归）；ItemsControl 类恒有子节点，例外以提前 `return` 跳过通用递归（ComboBox 折叠态） |
| ③ 设计时预览 | `TreeDrawListBuilder.as` | 与 ② 同构的类型分派 case（case 标签分组合法）；chrome 绘制前检查 `element.Children.Count > 0` 跳过文本 chrome——与 ② 语义等价的模板让位 |

**浮层体系（Popup 轨）**：弹出型 UI（ComboBox 下拉 / 后续 Tooltip、ContextMenu）统一经 `Popup`（`std/UI/Core/Components/Popup.as`）附加层，三轨同构闭合：同步轨 `PlatformTreeSync.BuildFromArc` 独立建树挂窗口平台根（非主树子节点）+ `RootEpoch` 代际守卫（跨会话句柄回收复用自愈）；渲染轨 `WgpuRender` `PopupLayer`/`PopupBackdrop` 分支置顶绘制；输入轨 `PointerRouter` 蒙层槽点击关闭 + 下拉类选项行经既有槽表路由。浮层回调一律**静态方法组 + 互斥槽锚点路由**（`_activeCombo` 型），禁实例方法组订阅（逃逸闭包 ByRef 捕获悬垂 UB，ItemsControl 同根因先例）。

**ItemsControl 数据面（单一惯用法 · 强类型）**：项集合来源唯一入口是 `ItemsSource` 属性（object 槽四分支判别：`string` / `List<string>` / `ObservableCollection<string>` / `ItemSourceView` 直行；null 与未知源清空，WPF `ItemsSource = null` 同语义），全部物化收敛为 `ItemSourceView` 数据源视图（`std/UI/Core/Components/ItemSourceView.as`）——**object 数据本体管道**（`Count` / `ItemAt` 返数据本体 / `DisplayAt` 返显示投影），对标 WPF「ItemsSource 承载任意对象」。视图三种构造轨：string 便捷轨（`From(string)`）、强类型静态轨（`From<T>(List<T>, Func<T,string>)` 与 `From<T>(EnumOptions<T>)`，投影在构造期烘焙——编译期类型检查，非运行期字符串反射路径）、动态轨（`From(ObservableCollection<string>)` 订阅迁入视图）。选中态（`SelectedItem`）与多选态（`SelectedItems`）承载**数据本体**而非显示字符串；派生控件读数据源做派生物化（如 ComboBox 下拉）一律经 `View` **共享同一视图实例**（`Detach()` 对静态轨 no-op，共享安全），不重开写面。`DisplayMemberPath` 已撤除（运行期字符串投影与强类型数据面背道而驰，投影职责并入视图构造期）；呈现定制唯一入口是 `ItemTemplate`（`DataTemplate` Instantiate/Recycle 委托对经 `ItemContainerGenerator` 模板路径物化，容器回收池复用）；**命令式 `Set*Items` 公开 API 已撤面**（双轨禁令，RFC 001 单一惯用法）。

**选择面（WPF Selector / MultiSelector 分层）**：集合类控件的选择语义按 WPF 同构分两层收敛——单选层 `Primitives.Selector`（`std/UI/Core/Components/Primitives/Selector.as`；派生链 Control → ItemsControl → **Primitives.Selector** → ListView / ComboBoxBase（→ ComboBox&lt;T&gt;），对标 WPF Control → ItemsControl → Selector）；多选层 `Primitives.MultiSelector`（`std/UI/Core/Components/Primitives/MultiSelector.as`；派生 Selector 承接全单选面，承载 SelectionMode/SelectedItems/SelectItem 多选扩展——多选语义载体，单选层不感知；DataGrid 派生此层，对标 WPF Primitives.MultiSelector → DataGrid）。选中流程唯一入口是单选层模板方法 `SelectIndex`（校验 → 写点 → 平台镜像同步 → 附加同步 → 通知），差异点经 protected virtual 钩子插拔：`SelectionItemCount`（条目上界，DataGrid 覆写为行数）/ `ApplySelectedIndexCore`（选中写点）/ `OnSelectionApplied`（附加同步，ListView 装箱 SelectedItem）/ `SelectionPayload`（载荷提取，DataGrid 覆写为行首列文本）/ `RaiseSelectionChanged`（通知触发，ComboBox&lt;T&gt; 覆写为 Signal&lt;T&gt;）/ `SyncMirrorSelection`（平台镜像双写）。**新增集合类控件一律派生对应层（单选 → `Primitives.Selector`，多选 → `Primitives.MultiSelector`）并只覆写钩子差异点，禁止重写选中流程或自建 SelectionChanged 通道**（双轨禁令，RFC 001）。自管视口特例（如 DataGrid 多列单元格）经 protected ctor `ownsItemsHost=false` 跳过基类项宿主装配，选择面照常继承。

**验收 checklist**（逐项核对，缺一不得宣称该控件生产就绪）：

- [ ] 三层门禁等价性：② `templated` 与 ③ 子树检查语义一致——模板化元素两层均不画内置 chrome
- [ ] 属性面跨层一致：控件类 DP ↔ `PlatformTreeSync` 属性同步注册 ↔ VSM 可查询状态（新增属性三处同补）
- [ ] 样式正道：渲染分支只读 DP / 主题资源键（`Controls.arml` 隐式 Style），禁止硬编码色（§4 样式契约同款）
- [ ] 交互态经 VSM internal 配方，不设 Style.Triggers 表达交互态（§1 双轨禁令）
- [ ] 可验证：ArmlDemo 全量编译（`cargo run -p arc -- build`）+ L1 语言批次通过；chrome/门禁行为有 L2 用例或有验收记录

---

[返回 037 主题入口(../../037-ui.md) · [references 索引](index.md)
