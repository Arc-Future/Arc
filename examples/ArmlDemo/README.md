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
| Hello | 落地页 + AppSans + Click / **`Command="{Binding Click}"`**（RelayCommand + CanExecute） | ✅ 可交互 | CanExecute → 按钮 IsEnabled |
| Controls | Style 短键多绑定 + Toggle/Check/Radio/Text/Password + Border 分区 + Popup/MessageBox | ✅ Style 权威变体 | PasswordBox：禁 Copy/Cut；允许 Paste |
| Bind | 产品 `{Binding Path}`：`Window.Title` · 普通属性 OneWay · `[Observable]` OneWay/TwoWay · `Model.Name` 叶通知 + 父替换 · Command / IsEnabled / Content | ✅ 数据驱动重画 | `{x:Bind}` 非作者 API；Content 为编译期 setter（非活订阅） |
| List | `ItemsSource="{Binding Items}"` + 选中 + 键盘导航 + 选中标签 | ✅ 有界高度 + 多项 | Observable 多实例并发 ✅ |
| Media | Image 并排 + Slider（Value 标签联动） | ✅ 拖拽/态色已接 | — |
| IME | 双 TextBox + IME/caret + Ctrl+C/V/X + 双击词选 | ✅ | 三击行选后置 |
| Style | keyed DangerButtonStyle vs 短键 / VisualHost | ✅ 非全局污染；禁 Class/Appearance | — |
| Data | CodeEditor + DataGrid ItemsSource | ✅ 选中/虚拟化 | Observable 行增量 + 多实例并发 ✅ |
| Tree | TreeView FlatIndex 视口（M-VZ4）+ 键盘 | ✅ 可点 + Tab + 视口池 | 非完整层级路径虚拟化 |
| Overflow×3 | 长 Header 挤栏 → 顶栏滚轮水平滚 | ✅ 溢出冒烟 | 无箭头 chrome 动画；关闭 = 产品 VSCode 行为（选中 × 常显 / 未选中悬停） |

**浮层**：Controls「Open Popup」/「Stacked Popup」与 ComboBox 下拉均走 Popup。**MessageBox**：OK / OKCancel / YesNo / YesNoCancel → `ShowAsync` + 自绘色块图标。

## Binding 产品面（Bind 页）

唯一作者惯用法 `{Binding Path}`（可选 `Mode=`）。code-behind 暴露同名属性；**不是**运行时 DataContext 路径行走。

| 标记 | 源 | 预期 |
|------|----|------|
| `Window.Title="{Binding Caption}"` | `[Observable] Caption` | Set title / Reset title 改原生标题 |
| `Text="{Binding Greeting, Mode=OneWay}"` | 普通 `Greeting` | Mutate 后 UI **保持加载快照** |
| `Text="{Binding Message, Mode=OneWay}"` | `[Observable] Message` | Change / Reset / Append 立即刷新 |
| `TextBox Text="{Binding Draft, Mode=TwoWay}"` | `[Observable] Draft` | 输入写回；Fill from code 回推盒子 |
| `Text="{Binding Model.Name}"` | `[Observable] Model` + `[Observable] Name` | Rename leaf 立刻刷新；Replace Model 重订 |
| `Command="{Binding Click}"` | `RelayCommand Click` | Execute + Toggle CanExecute |
| `IsEnabled="{Binding FeatureEnabled}"` | `[Observable] bool` | Toggle 后按钮禁用/恢复 |
| `Content="{Binding CommandLabel}"` | 普通 `CommandLabel` | 编译期 setter（初值） |
| `ItemsSource="{Binding Items}"` | `ObservableCollection<string>` | List 页集合 + OnLoaded 增量 |

## 运行

```text
cargo run -p arc -- build examples/ArmlDemo
examples/ArmlDemo/bin/Debug/ArmlDemo.exe
```

Win32 上应弹出 **880×720** 窗口：顶栏多页签（按文案测宽左对齐，选中 Accent 底线；总宽超出时顶栏滚轮水平滚）。页签关闭：选中「×」常显，未选中悬停才显。Esc：若有 Popup 轻关闭则关下拉，否则关主窗。

## 样式演示口径

`App.arml` 注册 `Demo.SectionTitle`（Primary）/ `Demo.Caption`（Text.Secondary）/ 显式 `DangerButtonStyle`；**不**注册全局隐式 Button colorError。Controls 用 **`Style="{StaticResource Primary, Medium}"`** 短键（RFC 037 §0.1.1；禁 Class/Appearance）。默认皮肤 **Ant Design 6.x 令牌对齐**（令牌对齐 ≠ DOM 像素复刻）。

## 已知挂账

- TabControl：无切换动画 / 溢出左右箭头动画；**关闭按钮 ✅**（VSCode：选中常显 / 未选中悬停）；**溢出滚动最小面 ✅**。
- 独立 ScrollBar 控件延后；ProgressBar `IsIndeterminate` ✅；PasswordBox 掩码 ✅；Border ✅。
- ComboBox 下拉钳高 ScrollView ✅；多弹层 Z 序 ✅。
- `ComboBox<T>`：`SetOptions` + `Enum.GetOptions<DemoThemeKind>()` 冒烟；ARML 仍 `ComboBoxBase`+ItemsSource。
- ListView / TreeView 键盘导航 ✅；TreeView FlatIndex 视口诚实边界见上表。
