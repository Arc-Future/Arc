# ArmlDemo

合并自分散 ARML UI 案例的**单一综合演示**。形态为 **`TabControl` 内置页签栏 chrome + 互斥分页**（仅展示 `SelectedIndex` 对应 `TabItem`）。

| 项 | 说明 |
|----|------|
| 权威 | [RFC 037](../../docs/rfc/037-ui.md) · 控件矩阵见 [COMPONENTS.md](../../std/UI/Core/COMPONENTS.md) |
| 角色 | 文档 / 演示；**非**默认硬绿权威；**非** Stable / 像素级 Ant 复刻宣称 |
| 前置 | Win32 + `clang` |

## 页签盘点（能力对接）

| 页签 | 内容 | 本轮状态 | 诚实边界 |
|------|------|----------|----------|
| Hello | 落地页 + AppSans + Click / **RelayCommand**（code-behind） | ✅ 可交互 | ARML `Command=` 绑定后置 |
| Controls | Style 短键多绑定 + Toggle/Check/Radio/Text/Password + Border 分区 + Popup/MessageBox | ✅ Style 权威变体 | PasswordBox：禁 Copy/Cut；允许 Paste |
| Bind | x:Bind OneWay + Change/Append（SyncText→Invalidate） | ✅ 数据驱动重画 | `{Binding}` 运行时后置 |
| List | ListView 选中 + 键盘导航 + 选中标签 | ✅ 有界高度 + 多项 | Observable 多实例并发 ✅ |
| Media | Image 并排 + Slider（Value 标签联动） | ✅ 拖拽/态色已接 | — |
| IME | 双 TextBox + IME/caret + Ctrl+C/V/X + 双击词选 | ✅ | 三击行选后置 |
| Style | keyed DangerButtonStyle vs 短键 / VisualHost | ✅ 非全局污染；禁 Class/Appearance | — |
| Data | CodeEditor + DataGrid ItemsSource | ✅ 选中/虚拟化 | Observable 行增量 + 多实例并发 ✅ |
| Tree | TreeView FlatIndex 视口（M-VZ4）+ 键盘 | ✅ 可点 + Tab + 视口池 | 非完整层级路径虚拟化 |
| Overflow×3 | 长 Header 挤栏 → 顶栏滚轮水平滚 | ✅ 溢出冒烟 | 无箭头 chrome / 无切换动画 |

**浮层**：Controls「Open Popup」/「Stacked Popup」与 ComboBox 下拉均走 Popup。**MessageBox**：OK / OKCancel / YesNo / YesNoCancel → `ShowAsync` + 自绘色块图标。

## 运行

```text
cargo run -p arc -- build examples/ArmlDemo
examples/ArmlDemo/bin/Debug/ArmlDemo.exe
```

Win32 上应弹出 **880×720** 窗口：顶栏多页签（按文案测宽左对齐，选中 Accent 底线；总宽超出时顶栏滚轮水平滚）。Esc：若有 Popup 轻关闭则关下拉，否则关主窗。

## 样式演示口径

`App.arml` 注册 `Demo.SectionTitle`（Primary）/ `Demo.Caption`（Text.Secondary）/ 显式 `DangerButtonStyle`；**不**注册全局隐式 Button colorError。Controls 用 **`Style="{StaticResource Primary, Medium}"`** 短键（RFC 037 §0.1.1；禁 Class/Appearance）。默认皮肤 **Ant Design 6.x 令牌对齐**（令牌对齐 ≠ DOM 像素复刻）。

## 已知挂账

- TabControl：无切换动画 / 关闭按钮 / 溢出左右箭头；**溢出滚动最小面 ✅**。
- 独立 ScrollBar 控件延后；ProgressBar `IsIndeterminate` ✅；PasswordBox 掩码 ✅；Border ✅。
- ComboBox 下拉钳高 ScrollView ✅；多弹层 Z 序 ✅。
- `ComboBox<T>`：`SetOptions` + `Enum.GetOptions<DemoThemeKind>()` 冒烟；ARML 仍 `ComboBoxBase`+ItemsSource。
- ListView / TreeView 键盘导航 ✅；TreeView FlatIndex 视口诚实边界见上表。
