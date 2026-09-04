# 内置主题资源（ResourceDictionary.arml）

> 本文是 [037 UI 声明式框架(../../037-ui.md) 的**渐进式披露子项**。承载「内置 Light/Dark 色值与控件隐式 Style 的 ARML 正道」。**未经验收不得宣称主题已完备**（宣称纪律）。

## 1. 目标与最终形态

| 面 | 设计（最终形态） |
|----|-----------------|
| 色值 | `std/UI/Core/Themes/*.arml` 声明 `ResourceDictionary`，编译期扁平化：`Light.arml`/`Dark.arml` 为色值权威源；`BuiltInTheme.Colors.g.as` 由 arc-ui 生成；`CreateLight`/`CreateDark` 调 `BuiltInThemeColors` + `FillNonColor` |
| 几何/深度/时长 | 代码常量（`CornerRadius` / `Elevation` / motion ms）；在 `BuiltInTheme` 结构化常量；**保持 AS** |
| 隐式控件 Style | 同主题 ARML / `Controls.arml` 中 `Style TargetType=…`：字体**不进**隐式 Style——FontFamily/FontSize/FontWeight/Foreground 为环境 DP（037 §4 属性值继承），全局字体默认单一源 = `Control` DP 默认值；`Controls.arml` 仅承载 chrome Setter（当前 Window 占位），`ThemeDictionary.AddImplicitStyles` 不承载字体 Setter |
| 应用覆盖 | `<Application.Themes>` / `<Application.Resources>`：codegen 扁平化 Themes（`BasedOn` → `BuiltInTheme.Create*`） |

**色值经 ARML 单一源**；禁止在 `BuiltInTheme.as` 再写色值 hex 双源。

## 2. 文件布局（正道）

```
std/UI/Core/Themes/
  Light.arml          # x:Key 色值 / 间距 / 字号等 ResourceDictionary
  Dark.arml           # 同 key 集，深色值
  Controls.arml       # 内置控件隐式 Style（TargetType，无 x:Key）
```

| 规则 | 说明 |
|------|------|
| 单一惯用法 | 内置色值只在 ARML 声明；`BuiltInTheme.as` 仅保留键名常量 + 几何/motion + 薄工厂 |
| 键集稳定 | 键名以 `BuiltInTheme` 的 `const string` 为权威（防拼写错）；ARML `x:Key` 必须与之逐字一致 |
| 编译期扁平 | 框架内置主题经构建/codegen **平坦**写入运行时字典；切主题 O(1) 换引用（既有 `ThemeDictionary.RegisterTheme`） |
| 用户覆盖 | 应用 `Application.Themes`/`Resources` 本地条目优先于活动主题（既有 MergedDictionaries 语义） |

## 3. 控件隐式 Style 范围（本生产门禁）

| TargetType | 最低 Setter 面 | 颜色 |
|------------|----------------|------|
| `Button` / `TextBox` / `CheckBox` / `ToggleButton` / `TextBlock` | chrome Setter（`Padding` 等 DP 存在者）；**字体禁入**——环境 DP 继承 + `Control` 默认值单一源 | Setter 色值用 `{StaticResource Color.*}`（应用期按活动主题解析，切主题经样式重应用刷新）；交互态反馈走 VSM→主题 token（渲染器每帧解析） |
| `ScrollView` | 无强制 Font；滚动条视觉走 `Color.Scroll.*` token | 同上 |
| `Window` | 可空 Style 占位 | — |

拒绝：把主题色 hex 字面量写死进隐式 Style 的 Setter（如 `Background="#3B82F6"`——切 Dark 失效）。主题色引用正道 = `{StaticResource Color.Primary}`：键编译期确定、值应用期按活动主题解析（主题即资源，经 `MergedDictionaries` 并入解析链），`SwitchTheme` 重新应用隐式样式全链刷新（详见 [037-ui.md §4(../../037-ui.md)）。

## 4. 落地约束（单一源纪律）

1. 新 token 必须先加 `BuiltInTheme` 键常量 + `Themes/*.arml` 条目，再 `UPDATE_BUILTIN_THEME=1` 再生 `BuiltInTheme.Colors.g.as`；**禁止**在 AS 内新增色值字面量。
2. 契约测试断言 ARML 源与生成物同步，维护 SwitchTheme 全链一致。

## 5. 非目标（本门禁）

- 第三方主题市场 / 运行时下载主题包
- 声明式 `VisualStateGroup` 全量（chrome 仍 VSM+RenderTree，见 [production-surface](production-surface.md)）
- `ControlTemplate` 树替换全部内置 chrome（后续能力；未立宪前禁止双轨）

---

[返回 037 主题入口(../../037-ui.md) · [references 索引](index.md)
