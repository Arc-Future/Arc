# 内置主题资源（ResourceDictionary.arml）

> 本文是 [037 UI 声明式框架](../../037-ui.md) 的**渐进式披露子项**。承载「内置 Light/Dark 色值与控件隐式 Style 的 ARML 正道」。**未经验收不得宣称主题已完备**（宣称纪律）。
>
> **双对齐口径（唯一）**：交互语义 / API / 布局对标 **WPF**；默认视觉皮肤对标 **Ant Design 6.x 公开 Seed/Map Token**（[customize-theme](https://ant.design/docs/react/customize-theme)：`colorPrimary=#1677ff`、`borderRadius=6`、`controlHeight=32`、`fontSize=14`、`colorBorderDisabled` 等）。**令牌对齐，不宣称像素级 DOM/CSS 复刻**。禁止 Fluent/Material 混搭作为默认色。架构总览与路线图见 [theme-style-interaction-architecture](theme-style-interaction-architecture.md)。

## 1. 目标与最终形态

| 面 | 设计（最终形态） |
|----|-----------------|
| 色值 | `std/UI/Core/Themes/*.arml` 声明 `ResourceDictionary`，编译期扁平化：`Light.arml`/`Dark.arml` 为色值权威源（Light=`defaultAlgorithm` map；Dark=`darkAlgorithm` 预烘焙快照）；`BuiltInTheme.Colors.g.as` 由 arc-ui 生成；`CreateLight`/`CreateDark` 调 `BuiltInThemeColors` + `FillNonColor` + `AddImplicitStyles`（Controls） |
| 几何/深度/时长 | 代码常量（`CornerRadius` / `Elevation` / motion ms）；在 `BuiltInTheme` 结构化常量；**保持 AS**（`borderRadius=6` / `borderRadiusLG=8` / `controlHeight=32`） |
| 隐式控件 Style | `Themes/Controls.arml` + `Themes/Controls/*.arml`：每控件真实 chrome Setter（`{StaticResource Color.*}` / `Size.*`）；字体**不进**隐式 Style——FontFamily/FontSize/FontWeight/Foreground 为环境 DP（037 §4）；全局字体默认单一源 = `Control` DP 默认值；`Foreground` 未设时渲染/VSM 回落活动主题 `Color.Text.Primary`；运行时 `CreateLight`/`CreateDark` → `AddImplicitStyles` → `MergedDictionaries.Add(CreateControls())`；`Application.ApplyStyleTree` 启动与 `SwitchTheme` 重应用 |
| 应用覆盖 | `<Application.Themes>` / `<Application.Resources>`：codegen 扁平化 Themes（`BasedOn` → `BuiltInTheme.Create*`） |

**色值经 ARML 单一源**；禁止在 `BuiltInTheme.as` 再写色值 hex 双源。

## 2. 文件布局（正道）

```
std/UI/Core/Themes/
  Light.arml                 # 色值权威（Ant 6 Map）
  Dark.arml                  # 同 key 集，深色值
  Controls.arml              # 聚合：MergedDictionaries → Shared + Controls/*
  Controls/
    Shared.arml              # 全局短键：Small/Medium/Large（MinHeight+Padding）+ Primary/Default/…
    Window.arml              # Window Background ← Color.Background
    Button.arml              # 隐式 BasedOn Medium（尺寸走 Shared；无 Button.Small 空覆写）
    ToggleButton.arml        # 隐式 BasedOn Medium
    CheckBox.arml            # 隐式 BasedOn Medium
    RadioButton.arml
    TextBox.arml            # 隐式 BasedOn Medium（Padding Setter 无 DP → 静默跳过）
    PasswordBox.arml
    Border.arml             # Surface/Border/Radius；Thickness/Padding→Size.Border.*
    ComboBox.arml
    ProgressBar.arml         # MinHeight→Size.Progress.Thickness（≠ controlHeight）
    Slider.arml              # MinHeight→Size.Slider.Height
std/UI/Core/Layout/
  ControlMetrics.as          # controlHeight / padding / radius / fontSize 数值权威
std/UI/Core/Styling/
  BuiltInTheme.Colors.g.as   # 色值生成物
  BuiltInTheme.Styles.g.as   # 隐式+keyed Style 生成物（Controls.arml 扁平）
```

| 规则 | 说明 |
|------|------|
| 单一惯用法 | 内置色值只在 ARML 声明；`BuiltInTheme.as` 仅保留键名常量 + 几何/motion + 薄工厂 |
| 键集稳定 | 键名以 `BuiltInTheme` 的 `const string` 为权威（防拼写错）；ARML `x:Key` 必须与之逐字一致 |
| 编译期扁平 | 框架内置主题经构建/codegen **平坦**写入运行时字典；切主题 O(1) 换引用（既有 `ThemeDictionary.RegisterTheme`） |
| 用户覆盖 | 应用 `Application.Themes`/`Resources` 本地条目优先于活动主题（既有 MergedDictionaries 语义） |
| 模块化 | `Shared.arml` 全局短键 + **每控件一个** `Controls/{Control}.arml`（隐式 `BasedOn Medium` 或独有 chrome；**仅差异**才写 `{T}.Small`）；**禁** `*.Styles.arml` / `*.Size.SM` 作者双轨；元素端 = `Style="{StaticResource Primary, Small}"`（§0.1.1；**禁 Class**） |
| 禁硬编码 | Controls `*.arml` Setter **禁**裸 `#hex`、裸 `N,N,N,N` Thickness；须 `{StaticResource Color.*}` / `Size.*` / `Radius.*`（契约 `controls_arml_no_bare_thickness_or_hex`） |
| 禁复制 MinHeight | **禁止**每个控件粘贴 `MinHeight={StaticResource Size.Control.Height}`；隐式默认 = Medium（BasedOn） |

### 内置控件 Style 对齐表（一眼）

| TargetType | 隐式 | 可绑短键 | 回退 |
|------------|------|----------|------|
| **Shared（全局）** | — | `Small`/`Medium`/`Large`（MinHeight+Padding）；`Primary`/`Default`/`Dashed`/`Text`/`Link`/`Danger`（色态标识，TargetType=Button） | — |
| Button | `BasedOn Medium`（无本地 Setter） | 色态 + 尺寸短键 | `{T}.K` → Shared `K`（当前无 `Button.Small` 等） |
| ToggleButton | `BasedOn Medium` | 尺寸短键 | 同上 |
| CheckBox / RadioButton | `BasedOn Medium` | 尺寸短键 | 同上 |
| TextBox / PasswordBox | `BasedOn Medium` | 尺寸短键 | Shared；Padding 无 DP → 跳过 |
| ComboBox | `BasedOn Medium` | 尺寸短键 | 同上 |
| Border | 独有 chrome（Surface/Border/Radius/Padding→`Size.Border.*`） | （无尺寸短键惯例） | — |
| ProgressBar | `MinHeight→Size.Progress.Thickness` | （轨高 ≠ controlHeight） | — |
| Slider | `MinHeight→Size.Slider.Height` | （含 thumb；≠ controlHeight） | — |
| Window | `Background→Color.Background` | — | — |
| TextBlock | 无隐式 chrome | — | 字体 = 环境 DP |

### 资源键命名规范（单一惯用 · 禁混用）

| 前缀 | 用途 | 层级示例 |
|------|------|----------|
| `Color.*` | Light/Dark 色值（ARML 权威） | `Color.Primary` · `Color.Text.Primary` · `Color.Scroll.Thumb.Hover` |
| `Size.*` | 尺寸 / Thickness | `Size.Control.Height[.SM\|.LG]` · `Size.Control.Padding*` · `Size.Border.*` |
| `Spacing.*` | 8-grid 间距标量 | `Spacing.XS`…`Spacing.XL` |
| `Radius.*` | 圆角 | `Radius.Control` / `Surface` / `Pill` |
| `Font.*` | 字号/族 | `Font.Body.Size` |
| `Motion.*` | 时长/缓动 | `Motion.Duration.Fast` · `Motion.Easing.Standard` |
| **Style 短键** | 作者面变体/尺寸（**另一命名空间**） | 裸 `Primary` / `Small` / `Danger`（≠ `Color.Primary`） |

**交互态后缀（唯一）**：语义角色用 `.Hover` / `.Pressed`（如 `Color.Primary.Hover`、`Color.Danger.Pressed`、`Color.Scroll.Thumb.Pressed`）。**禁用**走共享键 `Color.Disabled.Fill` / `Color.Disabled.Text`（跨角色 chrome；**不**写 `Color.Primary.Disabled`）。Light/Dark **同键集**。

**与 Style 短键分界**：`Color.Primary` / `Color.Danger` 是主题色资源；`Style="{StaticResource Primary, Small}"` 的 `Primary`/`Small` 是 Style 查找短键（§0.1.1：先 `{TargetType}.K` 再全局 `K`）。二者不得混写或互为别名。

### 几何 Thickness 镜像键（FillNonColor）

| 键 | 溯源 |
|----|------|
| `Size.Control.Height[.SM\|.LG]` | `ControlMetrics.ControlHeight*` |
| `Size.Control.Padding[.SM\|.LG]` | `ControlMetrics.ButtonPadding*`（Thickness 字符串） |
| `Size.Border.Thickness` | `ControlMetrics.BorderWidth` 四边 |
| `Size.Border.Padding` | `ControlMetrics.SpacingMD` 四边 |

### 渲染/占位色键（Light/Dark.arml）

| 键 | 用途 |
|----|------|
| `Color.Text.Selection` | TextBox 选区（Primary @ 25%） |
| `Color.Text.Placeholder` | 输入占位文字 |
| `Color.Image.Fill` / `Color.Image.Border` | Image 无纹理占位 |
| `Color.Text.Highlight` | DrawText 高亮底 |

## 3. 控件隐式 Style 范围（本生产门禁）

| TargetType | 最低 Setter 面 | 颜色 |
|------------|----------------|------|
| `Button` / `TextBox` / `PasswordBox` / `CheckBox` / `ToggleButton` / `TextBlock` | chrome Setter（`Padding` 等 DP 存在者）；**字体禁入**——环境 DP 继承 + `Control` 默认值单一源；未设 `Foreground` 不烤进镜像，渲染回落 `Color.Text.Primary` / VSM | Setter 色值用 `{StaticResource Color.*}`（应用期按活动主题解析，切主题经样式重应用刷新）；交互态反馈走 VSM→主题 token（渲染器每帧解析）；**Hover/Pressed/Disabled 覆盖静态 Background**（见 theme-style-interaction-architecture）；未设 Background（透明）不挡 VSM chrome |
| `ScrollView` | 无强制 Font；滚动条视觉走 `Color.Scroll.*` token | 同上 |
| `Window` | `Background={StaticResource Color.Background}` | — |

拒绝：把主题色 hex 字面量写死进隐式 Style 的 Setter（如 `Background="#3B82F6"`——切 Dark 失效）。主题色引用正道 = `{StaticResource Color.Primary}`：键编译期确定、值应用期按活动主题解析（主题即资源，经 `MergedDictionaries` 并入解析链），`SwitchTheme` 重新应用隐式样式全链刷新（详见 [037-ui.md §4](../../037-ui.md)）。

## 4. 落地约束（单一源纪律）

1. 新 token 必须先加 `BuiltInTheme` 键常量 + `Themes/*.arml` 条目，再 `UPDATE_BUILTIN_THEME=1` 再生 `BuiltInTheme.Colors.g.as`；**禁止**在 AS 内新增色值字面量。
2. 契约测试断言 ARML 源与生成物同步，维护 SwitchTheme 全链一致。
3. 默认色必须可追溯到 Ant Design **6.x** 公开 Seed/Map Token；改色须注明对应 Ant 键名。

## 5. 非目标（本门禁）

- 第三方主题市场 / 运行时下载主题包
- 声明式 `VisualStateGroup` 全量（chrome 仍 VSM+RenderTree，见 [production-surface](production-surface.md)）
- 完整 port Ant `darkAlgorithm` / `compactAlgorithm` 运行时（Dark 为预烘焙快照）
- Ant 组件级 token 全家桶（Button 实心/默认/虚线全变体）——有边界切片见 [theme-style-interaction-architecture](theme-style-interaction-architecture.md) P1–P3
- **像素级复刻 antd DOM/CSS**（能力与产品定位均否；见同文 §1）
- ARML 字面 `<ControlTemplate>` 发射进 Styles.g.as（默认模板本轮由 `DefaultControlTemplates` 代码工厂挂隐式 Style；字面发射后置）
- TabControl / DataGrid 专属 chrome 模板化（登记后置；Slider/ProgressBar/ComboBox 已迁）

---

[返回 037 主题入口](../../037-ui.md) · [references 索引](index.md)
