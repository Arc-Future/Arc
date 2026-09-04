# ArmlDemo

合并自 9 个分散 ARML UI 案例的**单一综合演示**，展示 Arc 声明式 UI（ARML）编码模型与能力面。`Application.Run()` 是单主窗口阻塞入口、无导航/TabControl/Visibility，故形态为**单 Window + ScrollView 内分区堆叠**。

| 项 | 说明 |
|----|------|
| 权威 | [RFC 037](../../docs/rfc/037-ui.md) · 控件矩阵见 [COMPONENTS.md](../../std/UI/Core/COMPONENTS.md) |
| 角色 | 文档 / 演示；**非**默认硬绿权威（原 `arc-integration` 硬绿已随该 crate 退场） |
| 前置 | Win32 + `clang`（原 opt-in e2e 前置条件；该 e2e 已随 arc-integration 退场 a2627a0f） |

## 合并来源（旧案例已删除）

| 分区 | 内容 | 原案例 |
|------|------|--------|
| 1 | Hello 元素树与事件（嵌套 StackPanel + Click 计数器） | ArmlHello |
| 2 | Controls 控件面（Rectangle / Primary/Secondary/Disabled Button / ScrollView 12 行 + ArmlStyle 默认 Input 观感） | ArmlControls + ArmlStyle |
| 3 | x:Bind 绑定（`Signal<string>` OneWay → Text 刷新） | XBindHello |
| 4 | ListView / ItemContainerGenerator（ItemsSource → Text 项） | ArmlList |
| 5 | Image（ImageDecoder 门面解码 → wgpu 纹理 `DrawTexture` 采样；解码失败回退占位框） | ArmlImage |
| 6 | Slider（Minimum/Maximum/Value + Foreground/IsEnabled） | ArmlSlider |
| 7 | IME / Input（英文直输 / 中文 IME + caret） | ArmlIme |
| 8 | Style & Isolation（宿主隐式 Button 红 vs VisualHost Light Primary 蓝） | ArmlVisualHost |
| 9 | CodeEditor 视口虚拟化（mmap piece-table + `RenderVirtualizedLines`） | ArmlCodeEditor |

## 运行

```text
cargo run -p arc -- build examples/ArmlDemo
examples/ArmlDemo/bin/Debug/ArmlDemo.exe
```

Win32 上应弹出 720×640 窗口：ScrollView 内 9 个分区依次堆叠（滚轮/滚动条纵向浏览）。关闭窗口或按 Esc 退出。

## 隐式样式全局性

`App.arml.as`（原 ArmlVisualHost 逻辑）在 `OnStartup` 向 `Application.Resources` 注册 **Button 隐式红样式（#FFCC2222）**——它是应用级隐式样式，**作用于宿主层全部 Button**（分区 2 的 Controls 按钮会被染红，属预期演示效果）。分区 8 `VisualHost` 内层构造时合并 RFC 037 Light Theme，内层 Button 呈现 Primary 蓝（#1677FF），**不**受宿主隐式样式渗透（RFC 037 样式隔离）。

## 已知挂账

- `x:Name` 字段访问未实现（codegen 跳过；RFC 026 M4+ 命名查找挂账）：分区 4/9 的 code-behind 用局部实例演示同款 API，ARML 中保留 `x:Name` 声明以标注意图。
- 分区 5 Image `Source` 路径相对项目根（`assets/logo.png`）；`missing.png` 用于演示解码失败占位。

## opt-in e2e（历史记录）

原 `crates/arc-integration/tests/arml_demo_build_e2e.rs` 已随 arc-integration
退场（a2627a0f）：`#[ignore]` opt-in，不随默认矩阵运行；断言
`arc build examples/ArmlDemo` 成功、`bin/Debug/ArmlDemo.exe` 存在（不启动 GUI）。
