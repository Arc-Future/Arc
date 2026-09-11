# Arc.UI 主题 · 样式 · 交互响应架构（行业组件库水准路线图）

> 本文是 [037 UI 声明式框架](../../037-ui.md) 的**渐进式披露子项**。综合代码 + RFC + 业界（Ant Design 6 / WPF Fluent ResourceDictionary / 通用 Token 分层）做法，给出**唯一产品口径、现状病灶、目标分层与可执行切片**。**未经验收不得宣称「行业优秀组件库已达成」**（宣称纪律）。
>
> 关联：[builtin-theme-resources](builtin-theme-resources.md) · [production-surface](production-surface.md) · [ai-native-fidelity-loop](ai-native-fidelity-loop.md) · [plan.md](../../../plan.md)

## 0. 三句话结论

1. **产品口径（唯一 · 禁双轨）**：交互语义 / API / 布局对标 **WPF**；默认视觉皮肤对标 **Ant Design 6.x 公开 Seed/Map Token**——**令牌对齐，不宣称像素级 DOM/CSS 复刻**。
2. **架构正道**：`Themes/*.arml`（色值权威）→ `BuiltInTheme` 键 + 几何/motion → VSM 状态配方 → 渲染器按资源键解析 + MotionEngine 插值；交互态**禁止** Style.Triggers / 硬编码 hex。
3. **达行业水准的杠杆顺序**：Token 单一真相源 → 状态链路可靠（Pointer→镜像→VSM→wgpu）→ Binding 失效闭环 → 控件 chrome 模板化/变体 → 脏区与动效质量；**禁止**继续在 RenderTree 堆色值修补。

### 0.1 样式变体单一口径（RFC 037 §3 · 防文档/实现漂移）

| 维 | 唯一口径 |
|----|----------|
| **作者面** | **仅短键**多资源绑定：`Style="{StaticResource Primary, Small}"`（多键依次应用、**后覆盖前**） |
| **禁** | `Class` / `StyleClass` / `SizeMode` / `Variant` / `Appearance=` / **Appearance DP** / 作者面写 `Button.Size.SM` 等第二套命名 |
| **chrome 配方** | `AppliedStyleKeys` → VSM（变体可为无 Setter 的标识 Style） |
| **交互态** | VSM = Style **内部**引擎（§0.2）；禁 Triggers 写 Hover/Pressed |
| **字体** | 环境 DP；禁进隐式 Style |
| **资源模块化** | `Controls/Shared.arml`（全局短键；尺寸成套 MinHeight+Padding）+ `Controls/{Control}.arml`（隐式 `BasedOn Medium` 或独有 chrome；**仅差异时**写 `{T}.Small` 覆写）；禁 `*.Styles.arml` |

文档 / COMPONENTS / ArmlDemo / 本文件同口径；门禁 `style_variant_docs_match_rfc037`。

### 0.1.1 Style 键标准（一眼表 · 查找与复用）

作者**只写短键**。字典为避免平坦 RD 撞名，控件覆写存为 `{TargetType}.{Short}`；全局复用存为 `{Short}`。

| 项 | 规则 |
|----|------|
| **键形态** | **短键**（作者唯一）：`Primary` / `Small` / `Medium` / `Large` / `Danger`…。**作用域键**（仅资源文件 / BasedOn 内部）：`{T}.Small` = 控件对 `Small` 的覆写（**仅 Setter 与 Shared 不同时**才声明）。 |
| **查找顺序** | 对元素 `TargetType=T`、请求键 `K`：① 若 `K` 已含 `.` → 只查精确 `K`；② 否则先查 **`T.K`**（控件专用）；③ 再查 **`K`**（全局）；④ 均无或 `TargetType` 不匹配 → **失败**（诊断列出已试键）。 |
| **多键合成** | `Style="{StaticResource A, B}"`：先 A 后 B，**后覆盖前**（与既有多绑定一致）。 |
| **隐式 Style** | 无 `x:Key`、按 `TargetType` 自动套用；与显式短键正交——隐式打底，显式短键叠加/覆盖。**默认尺寸 = Medium**（控件隐式 `BasedOn="{StaticResource Medium}"`；**禁**每控件复制 `MinHeight=Size.Control.Height`）。 |
| **复用** | 共用尺寸/色态 → `Shared.arml` 的全局 `Small`/`Medium`/`Large`/`Primary`…（尺寸键含 MinHeight+Padding）；某控件要**不同** Setter → 同名短键的作用域键。**禁止**复制粘贴出 `Button.Size.SM` 第二命名，也禁止无差异的 `{T}.Small` 空覆写。 |
| **单一惯用** | 作者面只有短键；**不设** Class；**不设** `*.Size.SM` 作者 API。 |

```
作者: Style="{StaticResource Primary, Small}"   （Button）
         │
         ├─ Primary → 试 Button.Primary → 未注册
         │            再试 Primary（Shared 全局色态标识）
         └─ Small   → 试 Button.Small   → 未注册（与 Shared 同 Setter）
                      再试 Small（Shared：MinHeight+Padding）
```

实现：`StyleKeyResolver`（运行时）与 `arc-ui::style_key`（codegen/verify）**必须**同序；codegen 对主题域键保留短键字符串，由应用期走完整回退链（禁止编译期只展开成 `Button.Size.SM` 而跳过全局回退）。

### 0.2 VSM 审查裁决（单一惯用 · 防双心智）

**结论：VSM 合理，且必须降为 Style 的实现细节——不是与 Style 并行的第二套「样式/态外观」作者面。**

| 问 | 裁决 |
|----|------|
| Style 是否唯一惯用？ | **是**。默认 chrome、变体、尺寸、主题覆盖 → 隐式 Style / 短键显式 Style / 多绑定 / BasedOn。 |
| VSM 定位？ | **内部交互态引擎**；非作者 API。 |
| Appearance？ | **已删除**；chrome ← `AppliedStyleKeys`。 |
| 作者怎么选按钮形态？ | **只用** `Style="{StaticResource Primary, Large}"`。 |

## 1. 「跟 antd 一模一样」裁决

| 维 | 裁决 |
|----|------|
| **能力** | Arc.UI = **wgpu 矢量/SDF chrome + ARML/VSM**，不是浏览器 DOM。无法原生复刻 antd 的 CSS-in-JS、`@container` 条件、图标字体、复杂伪类、CSS 变量级联与 React 合成事件。圆角算法 / 运动曲线 / 子像素文字可**逼近**，不能承诺像素 diff 为零。 |
| **必要** | **不必要**。Arc 定位是桌面 AOT「WPF 能力面 + Ant 令牌皮肤」，不是 antd React 的像素克隆。像素克隆会锁死在 React DOM 实现细节，与 wgpu 唯一后端冲突。 |
| **推荐口径** | **令牌对齐 Ant Design 6.x**（Seed/Map 公开表：`colorPrimary=#1677ff`、`borderRadius=6`、`controlHeight=32`、`colorBorderDisabled` 等）；组件默认观感「同族可辨」；**文档与 COMPONENTS 禁止出现「像素级复刻 antd」表述**。 |

Ant Design 6.x 相对 5.x：主线是 **CSS Variables 默认 / zeroRuntime / 语义 DOM 清理 / `colorBorderDisabled` 等 Map Token**——Seed 主色与圆角/控件高与 5.x 公开默认值基本连续。Arc 侧升级 = **版本口径 + 可对齐 Map Token 补齐**，不是重画一套皮肤。

## 2. 主题样式系统架构（现状）

```
┌─────────────────────────────────────────────────────────────────┐
│ Application.Current（唯一解析根）                                  │
│  Resources 本地条目  >  MergedDictionaries[Active Theme]         │
│  SwitchTheme → SyncActiveTheme + VisualHost.ApplyAllHostStyles   │
│              + FramePump.Invalidate                              │
└────────────┬────────────────────────────────────────────────────┘
             │
   ┌─────────▼──────────┐     ┌──────────────────────────────────┐
   │ ThemeDictionary    │     │ Style / 隐式 Style                │
   │ Light|Dark → RD    │     │ Controls.arml + App.Resources     │
   │ BuiltInTheme 工厂  │     │ StyleEvaluator / StyleManager     │
   └─────────┬──────────┘     │ 优先级：本地值 > 样式 Setter > 继承 │
             │                └──────────────┬───────────────────┘
             │ 键 Color.* / Radius.*         │ Template Setter → ControlTemplate
             ▼                               ▼
   ┌─────────────────────┐     ┌──────────────────────────────────┐
   │ VisualStateManager  │     │ 环境 DP 继承（Font*/Foreground）   │
   │ ControlState→配方   │     │ RegisterInheritedProperty 推送   │
   │ → 资源键 + motion   │     └──────────────────────────────────┘
   └─────────┬───────────┘
             │ 渲染器每帧 ResolveColor(key)
             ▼
   ┌─────────────────────┐
   │ WgpuRender.RenderTree│ + MotionEngine 插值
   │ （模板让位：有视觉子树则跳过内置 chrome）│
   └─────────────────────┘
```

| 部件 | 路径 | 职责 |
|------|------|------|
| 色值权威 | `std/UI/Core/Themes/Light.arml` / `Dark.arml` | Ant Map Token 烘焙；`x:Key` ≡ `BuiltInTheme` const |
| 生成物 | `BuiltInTheme.Colors.g.as` / `Styles.g.as`（`UPDATE_BUILTIN_THEME=1`） | ARML→运行时填色/隐式 Style；禁手改 |
| 键/几何/motion | `std/UI/Core/Styling/BuiltInTheme.as` | 键常量 + `CornerRadius`/`Elevation`/`Motion*Ms` |
| C 镜像 | `crates/runtime-ui/platform/common/rt_ui_design_tokens.h` | Light 默认宏；契约测试对齐；**非第二权威** |
| 隐式 Style | `Themes/Controls.arml` + `Controls/*.arml` → Styles.g.as | chrome Setter；**字体禁入**；CreateLight/Dark MergedDictionaries 并入 |
| 主题持有 | `ThemeDictionary.as` | 名→平坦 `ResourceDictionary`；切主题 O(1) |
| 状态配方 | `VisualStateManager.as` | Hover/Pressed/Focus/Disabled/Checked **唯一**交互态正道 |
| 样式评估 | `StyleEvaluator` / `StyleManager` | 隐式/显式两趟；Triggers **禁**表达交互态 |
| 模板 | `ControlTemplate` + RenderTree `templated` 让位 | 有视觉子树则禁内置 chrome 双轨 |

**优先级（与 037 §4 一致）**：本地 DP 值 > 样式 Setter > 最近祖先环境有效值 > DP 默认。主题色引用唯一写法 `{StaticResource Color.*}`——**不设** `{ThemeResource}` / `{DynamicResource}`；切主题靠换活动字典 + 重应用隐式样式 + VSM 每帧按键解析。

## 3. 样式资源如何分布（单一真相源）

| 资源类 | 唯一拥有者 | 禁止 |
|--------|------------|------|
| 色值 hex | `Themes/*.arml` | `BuiltInTheme.as` / RenderTree / COMPONENTS 硬编码第二源 |
| 键名 | `BuiltInTheme` `const string` | ARML `x:Key` 拼写漂移；前缀分层见 [builtin-theme-resources §命名规范](builtin-theme-resources.md)（`Color.*` ≠ Style 短键 `Primary`） |
| 几何 / 时长 / Elevation | `BuiltInTheme.as` 结构化常量 | 控件内魔法数（新代码） |
| 控件 chrome 默认态 | VSM 配方键 +（可选）`Controls.arml` Setter | Style.Triggers 写 Hover |
| 交互态色 | VSM → 主题键；Hover/Pressed/Disabled **覆盖**静态 Background（WPF VisualState 语义） | `StateColor` 永久让静态 Background 挡态色 |
| 平台 Light 宏 | `rt_ui_design_tokens.h` = ARML 契约镜像 | 头文件单独改色 |
| 应用覆盖 | `Application.Resources` / Themes | 与内置键冲突的 Fluent/Material 默认混搭 |

生成链：`Light.arml`（Seed/Map）→ **`dark_map_derive`（构建期）** → `Dark.arml` → `Colors.g.as`；`Controls.arml`+`Controls/*` → Styles.g.as；契约：`design_tokens_contract`（含 `dark_arml_matches_light_seed_derivation`）/ `theme_switch_contract`。再生：`scripts/ui-theme/derive-dark-from-light.ps1`。

## 4. 组件响应链路

### 4.1 指针 / 键盘 → 视觉

```
Win32 WM_MOUSE* / KEY
  → pointer_win32 / keyboard 路由
  → elem->is_mouse_over / is_pressed（C）
  → rt_ui_dispatch_button_visual_state / control_*
  → PointerRouter.RouteVisualState（镜像 IsMouseOver/IsPressed + FramePump.Invalidate）
  → 可选 ApplyPointerState → Arc DP
  → 下一帧 WgpuRender：读镜像 → ControlState → VSM → ChromeStateColor / StateColorMotion
  → DrawRoundedRect / Shadow / Text
```

点击：`WM_LBUTTONUP` 命中原按下 Button → `rt_ui_dispatch_button_click` → `PointerRouter.RouteClick` → `Button.RaiseClick`（`IsEnabled` 门控）→ `Clicked` Signal / codegen `OnClick`。

键盘：`FocusManager` Enter/Space → `InputElement.Activate` → Button `RaiseClick`。

**句柄表**：`PointerRouter` 使用 `Dictionary<long, T>`（无固定 16 槽上限）——多页签 ArmlDemo 等场景可并存注册。

### 4.2 `{Binding}` → 刷新

```
[Observable] setter → 合成通道 Signal.Set
  → BindingOperations.BindText / SetBinding 订阅
  → SyncText：逻辑树 Text + ElementSetString 镜像 + FramePump.Invalidate
  → wgpu 下一帧读镜像 Text
```

正道：`{Binding Path}` 编译期脱糖（`this.Path` / `this.Foo.Bar`；仅 `[Observable]` 才 `ObserveProperty`。`[Observable]` = 通知 / TwoWay，不是绑定准入；中间有标才重订阅叶）。`{x:Bind}` 非作者 API。逃逸闭包约束：订阅回调只捕获绑定 id（见 `BindingOperations.as`）。

**命令面**：`Command="{Binding Click}"` → Command setter；`RaiseClick` → `ICommand.Execute`；`CanExecuteChanged` → IsEnabled。**不**宣称 Converter / 运行时路径行走 / CommandManager.RequerySuggested / IsEnabledCore 合取。

## 5. 现状病灶表（诚实）

| ID | 病灶 | 证据路径 | 影响 | 状态 |
|----|------|----------|------|------|
| F1 | 显式 Background（宿主 Style）永久挡住 VSM Hover/Pressed | 旧 `StateColorMotion`；`App.arml` 全局 Button `#FF4D4F` | 「无悬停反馈」假死 | **本轮已修**：`ChromeStateColor(forceTheme)` |
| F2 | `SyncText` 不 Invalidate | `BindingOperations.SyncText` | Binding 改数据不重画 | **本轮已修** |
| F3 | PointerRouter 共享 ≤16 槽，后注册控件丢 Click | 旧 fixed-slot | Bind/后页签按钮无响应 | **本轮已修**：Dictionary |
| F4 | 文档/COMPONENTS 仍写「Ant 5.x」 | 多处 | 口径过期 | **本轮升 6.x** |
| F5 | 缺 `colorBorderDisabled` Map Token | Ant 6 公开表 | 禁用描边无语义键 | **本轮已补** |
| F6 | RenderTree 控件分支硬编码布局魔法数 | `WgpuRender.RenderTree.as` | 与 token 几何脱节 | **已收敛本刀**：Slider/ProgressBar/Combo/焦点环/滚动条拇指/chevron 微几何/Tab 栏高·字号·指示条→ControlMetrics；DataGrid 行高→ControlMetrics；余量按触点继续扫 |
| F7 | 整树 `FramePump.Invalidate` | 无脏区矩形 | 大树交互掉帧 | **最小面已立**：`InvalidateRegion` + LoadOp_Load + 根 scissor；caret 区；控件级精确失效树后置 |
| F8 | Button 变体（default/dashed/text/link）未立 | VSM 仅 Primary/Ghost | 难达 antd 控件族完备 | **本轮已立** keyed Style：`Button.Primary\|…` + 短键 `Primary, Small`；`AppliedStyleKeys`→VSM（**禁 Class/Appearance DP**） |
| F9 | Dark 曾为手写预烘焙快照，无 `darkAlgorithm` 运行时 | `Dark.arml` 注释 | 改 Seed 不自动派生 Dark | **本刀：构建期派生 ✅**（`arc-ui::dark_map_derive` + `scripts/ui-theme/derive-dark-from-light.ps1`：Light Seed → Dark.arml → Colors.g.as）；**完整 darkAlgorithm 运行时仍 P3（非阻塞）** |
| F10 | 组件级 token 全家桶未立 | builtin-theme §5 非目标 | 深定制靠覆盖全局键 | 有边界后置 |
| F11 | Motion 曲线未对 Ant ease | `Motion*Ms` 仅时长 | 手感差距 | **已立** `Motion.Easing.*` token + EaseProgress |

## 6. 目标架构（行业优秀组件库分层）

对标业界通识 **Seed → Map → Alias → Component**（Ant）与 WPF **变量 RD → 语义刷 → 控件 Style/Template + VSM**：

| 层 | Arc 落点 | 内容 |
|----|----------|------|
| L0 Seed（意图） | 文档化；值烘焙进 ARML | `colorPrimary`、`borderRadius`、`controlHeight`、`fontSize`… |
| L1 Map（派生） | `Light/Dark.arml` | Hover/Active/Bg/Border/Disabled…（含 `colorBorderDisabled`） |
| L2 Alias / 语义 | `BuiltInTheme` 键 | `Color.Primary` / `Text.Primary` / `Border.Disabled`… |
| L3 控件主题 | VSM 配方 + 将来 ComponentToken 字典 | PrimaryButton / Ghost / Danger… |
| L4 状态 | `ControlState` + 镜像 bool | Hover/Pressed/Focused/Disabled/Checked |
| L5 动效 | `MotionEngine` + 时长/曲线 token | 跟手 hover、从容 focus |
| L6 模板 | `ControlTemplate` 优先（`DefaultControlTemplates` + PART_*）；无模板宿主硬分支仅回退 | 换皮不改控件类；禁新增已迁控件宿主 chrome |

**与产品口径统一**：L0–L2 数值溯源 Ant 6；L3–L6 交互与 API 溯源 WPF。渲染始终 wgpu。

## 7. 可执行路线图（分阶段 · 非假开全家桶）

### P0 — 架构对齐第一刀

- [x] 口径升 **Ant Design 6.x 令牌对齐 / 非像素克隆**（本文 + builtin-theme + COMPONENTS + plan）
- [x] 补 `Color.Border.Disabled`；再生 Colors.g.as；C 头对齐
- [x] Button chrome：交互态 VSM 覆盖静态 Background
- [x] Binding：`SyncText` → `FramePump.Invalidate`
- [x] PointerRouter：Dictionary 句柄表；Click `IsEnabled` 门控
- [x] 验收：`cargo test -p arc-ui`；`arc build examples/ArmlDemo`；冒烟 ≥15s

### P1 — 控件族可感知一致（本轮）

- [x] Toggle/CheckBox/Radio：与 Button 同构 `ChromeStateColor`（显式底色不挡 Hover/Pressed/Checked）
- [x] Button 语义变体：Primary / Default / Dashed / Text / Link / Danger（**作者面 = Style 短键/限定键多绑定** `Style="{StaticResource Primary, Large}"` → AppliedStyleKeys → VSM → 镜像 → wgpu；**禁 Class/Appearance DP**）
- [x] 尺寸令牌模块化：`ControlMetrics`（controlHeight SM/MD/LG + padding）+ BuiltInTheme.FillNonColor / LayoutHelper / RenderTree 同源；尺寸经 `*.Size.*` keyed Style（短键 Small/Medium/Large 按 TargetType 展开）
- [x] 样式资源模块化：`Themes/Controls/{Button,ToggleButton,…}.arml`（**每控件一文件**含隐式+keyed；禁 `*.Styles.arml`）+ `Controls.arml` MergedDictionaries；`CreateLight`/`CreateDark` → `AddImplicitStyles`；`ApplyStyleTree` 启动/切主题重应用
- [x] ArmlDemo：Controls/Bind/Style 页用 Style 多绑定展示变体与尺寸；撤全局隐式 error Style
- [x] Focus：`IsFocused` FocusRing 已接；**`IsFocusVisible` 键盘模态闭环**（Tab/方向显环 · 指针聚焦清环；caret 仍跟 IsFocused）

### P2 — 生产手感

1. ~~脏区：控件级 invalidate 矩形（替代整客户区）~~ → **最小面 ✅**（`InvalidateRegion` + LoadOp_Load + 根 scissor；caret）；控件级精确失效树后置
2. ~~Motion 曲线 token（对标 Ant `motionEaseOut` 子集）~~ → **✅**
3. ~~几何魔法数收敛余量（Slider/Tab 等剩余硬编码）~~ → **✅**（ControlMetrics 再扫；Golden `control_metrics_owns_geometry`）
4. ~~控件 Golden + DesignTokenCatalog 无裸值门禁加严~~ → **最小硬门槛 ✅**（DesignTokenCatalog + bare_value typeck + Button/TextBlock 布局 Golden）；控件×主题态 Golden 全集 / 审视回路仍后置

**登记后置**：DataGrid 列拖拽排序 / 单元格编辑（面过大，本轮跳过）。

### P3 — 有边界增强（须单独立项）

- 组件级 token 字典（Button 虚线/链接）——**非** antd 全家桶一次性移植
- `darkAlgorithm` / `compactAlgorithm` **运行时**派生（构建期 Seed→Dark Map 快照派生已立，见 F9；运行时仍须单独立项）
- 声明式 VisualStateGroup 全量（嵌 Template 内；与 internal VSM 禁双轨，须 RFC）
- ARML `<ControlTemplate>` 字面发射（本轮代码工厂权威）
- TabControl / DataGrid 专属 chrome 模板化（本轮诚实未迁：Panel 内容子树 / ApplyTo 清行）

### P1.5 — 控件 chrome 模板化（本轮）

- [x] 契约：默认模板 = `DefaultControlTemplates` → 隐式 Style Template Setter；部件 `PART_Chrome` / `PART_Glyph` / `PART_Content`；VSM 经 `ChromeHostHandle`+`ChromeRole` 画到 PART；宿主有子树跳过内置 chrome
- [x] 已迁：Button / ToggleButton / CheckBox / RadioButton / TextBox / PasswordBox（输入文本层仍宿主）/ Slider / ProgressBar（含 IsIndeterminate）/ ComboBox（折叠壳 PART；SelectedText/chevron 宿主层）
- [x] 未迁（诚实）：TabControl（Panel + TabItem 内容子树）/ DataGrid 专属 `RenderDataGrid`（ApplyTo 清子树毁行；面过大）
- [x] 验收：`cargo test -p arc-ui`；ArmlDemo build + ≥15s 冒烟

## 8. 验证矩阵（本主题变更）

| 变更 | 最低验证 |
|------|----------|
| 令牌 / ARML / Colors.g | `cargo test -p arc-ui --test design_tokens_contract` · `theme_switch_contract` |
| 样式/VSM/渲染 | `cargo test -p arc-ui`；ArmlDemo 悬停/点击/Bind 页手测 |
| 卫生 | 无仓库根调试产物；wgpu 唯一 |

## 9. 改样式去哪（P1 路径表）

| 要改什么 | 路径 |
|----------|------|
| 色值 hex | `Light.arml`（Seed/Map）→ `scripts/ui-theme/derive-dark-from-light.ps1` → `Dark.arml` → Colors.g；禁手改 Dark 为第二权威 |
| 尺寸/圆角/字号数值 | `std/UI/Core/Layout/ControlMetrics.as`（`BuiltInTheme.FillNonColor` 镜像进字典） |
| Button 隐式 + Shared 尺寸 | `Shared.arml` 全局 Primary/Small…（成套）+ `Button.arml` 隐式 `BasedOn Medium`；作者 `Style="{StaticResource Primary, Small}"`（§0.1.1；**禁 Class/Appearance/Size.SM**；无差异不设 `Button.Small`） |
| Toggle/Check/Radio/TextBox/PasswordBox | 同文件内 `*.Size.SM\|MD\|LG` keyed Style（`Themes/Controls/{Control}.arml`） |
| ToggleButton chrome | `Themes/Controls/ToggleButton.arml` |
| CheckBox chrome | `Themes/Controls/CheckBox.arml` |
| RadioButton chrome | `Themes/Controls/RadioButton.arml` |
| TextBox chrome | `Themes/Controls/TextBox.arml` |
| PasswordBox chrome | `Themes/Controls/PasswordBox.arml` + Size keyed |
| Border chrome | `Themes/Controls/Border.arml`（Surface/Border/Radius.Control） |
| ComboBox chrome | `Themes/Controls/ComboBox.arml` |
| ProgressBar chrome | `Themes/Controls/ProgressBar.arml` |
| Slider chrome | `Themes/Controls/Slider.arml` |
| Window chrome | `Themes/Controls/Window.arml` |
| 交互态配方 | `Styling/VisualStateManager.as`（禁 Style.Triggers） |
| 变体 API | **Style 多绑定 + TargetType 短键**（权威）；AppliedStyleKeys → VSM（实现细节）；**禁 Class/StyleClass/SizeMode/Appearance**；禁属性名 `Variant`（RFC 004） |

---

[返回 037](../../037-ui.md) · [references 索引](index.md) · [builtin-theme-resources](builtin-theme-resources.md) · [production-surface](production-surface.md)
