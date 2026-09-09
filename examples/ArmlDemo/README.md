# ArmlDemo

合并自分散 ARML UI 案例的**单一综合演示**。形态为 **`TabControl` 内置页签栏 chrome + 互斥分页**（仅展示 `SelectedIndex` 对应 `TabItem`）。

| 项 | 说明 |
|----|------|
| 权威 | [RFC 037](../../docs/rfc/037-ui.md) · 控件矩阵见 [COMPONENTS.md](../../std/UI/Core/COMPONENTS.md) |
| 角色 | 文档 / 演示；**非**默认硬绿权威 |
| 前置 | Win32 + `clang` |

## 页签盘点（能力对接）

| 页签 | 内容 | 本轮状态 | 诚实边界 |
|------|------|----------|----------|
| 1 Hello | 元素树 / 自定义字体 / Button Click（内容尺寸 chrome） | ✅ 可交互 | — |
| 2 Controls | Button Style 多绑定（**Left 内容尺寸**）+ Toggle/Check/Radio/TextBox/PasswordBox **Size keyed Style** + Border 装饰 + VSM；Progress/Combo/Popup/Scroll/MessageBox | ✅ Style 权威变体 | PasswordBox：禁 Copy/Cut；允许 Paste |
| 3 Bind | x:Bind OneWay + Change/Append Message（SyncText→Invalidate） | ✅ 数据驱动重画 | — |
| 4 List | ListView 选中 + **键盘导航**（↑↓ Home/End/Enter） | ✅ 有界高度 + 多项 | Observable 多实例并发 ✅ |
| 5 Media | Image + Slider（Value 标签联动） | ✅ 拖拽/态色已接 | — |
| 6 IME | 双 TextBox + IME/caret + **Ctrl+C/V/X** + **双击词选** | ✅ | 三击行选后置 |
| 7 Style | 显式 keyed DangerButtonStyle vs 短键 `Danger` / `Primary, Large`（亦可限定键）vs VisualHost | ✅ 非全局污染；禁 Class/Appearance | — |
| 8 Data | CodeEditor + DataGrid ItemsSource | ✅ 选中/虚拟化 | Observable 行增量 + 多实例并发 ✅ |

**浮层**：Controls 页「Open demo Popup」与 ComboBox 下拉均走 Popup M1（`Open(owner)` / 蒙层轻关闭 / Esc）。**MessageBox**：「OK」/「OKCancel」/「YesNo」/「YesNoCancel」→ `ShowAsync` + `MessageBoxImage` 自绘色块图标（Popup `IsLightDismissEnabled=false`；Esc 自管；禁原生对话框）。

## 运行

```text
cargo run -p arc -- build examples/ArmlDemo
examples/ArmlDemo/bin/Debug/ArmlDemo.exe
```

Win32 上应弹出 720×640 窗口：顶栏 8 个内置页签（**按文案测宽左对齐**，选中 Accent 底线）。Esc：若有 Popup 轻关闭则关下拉，否则关主窗。

## 样式演示口径（P1）

`App.arml` **不再**注册全局隐式 `Button` colorError。显式 `DangerButtonStyle` / `Demo.SectionTitle` 仅 Style/节标题引用；Controls 页用 **`Style="{StaticResource Primary, Medium}"`**（短键；查找先 `{T}.K` 再 Shared 全局；Shared `Small`/`Medium`/`Large` 成套 MinHeight+Padding；隐式默认 = Medium/`BasedOn`；RFC 037 §0.1.1；**禁 Class/Appearance/`*.Size.SM`**）。默认皮肤 **Ant Design 6.x** 令牌对齐。

## 已知挂账

- TabControl：无切换动画 / 溢出滚动页签 / 关闭按钮。
- 独立 ScrollBar 控件延后（竖条嵌于 ScrollView；命中在 C `rt_ui_vscroll_*`）。
- ProgressBar `IsIndeterminate` 扫掠 ✅；PasswordBox 掩码 ✅（禁 Copy/Cut；允许 Paste）；Border ✅（非均匀描边绘制取 max）；TextBox/PasswordBox **剪贴板** ✅；**双击词选** ✅。
- ComboBox 下拉 **钳高 ScrollView 外壳** ✅；多弹层 Z 序后置。
- `ComboBox<T>`：`SetOptions` + `Enum.GetOptions&lt;DemoThemeKind&gt;()` 烘焙 count=3 冒烟；ARML 控件仍 `ComboBoxBase`+ItemsSource。
- `IsFocusVisible`：Tab/方向键显示焦点环；点击 TextBox 等指针聚焦清环（caret 仍跟 `IsFocused`）。
- ListView：**键盘导航**（↑↓/Home/End/Enter → Selector）✅。