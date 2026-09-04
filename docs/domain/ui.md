# Arc.UI

## 概述

`Arc.UI` 是 Arc 的声明式界面框架。它以 **ARML** 标记语言描述界面树，采用 **WPF 对齐**的心智模型（标记 + code-behind + 强类型 `DataContext`），以**数据绑定 + `[Observable]` 通知**驱动更新，以 **wgpu** 为唯一 GPU 渲染后端，以**虚拟化**承载大数据量列表，以**主题**（Light/Dark）统一视觉。它面向桌面 AOT 优先，走单一惯用法——同一意图一条正道。

本分册讲如何使用 `Arc.UI` 开发界面；绑定机制、表达式树等语言级能力见规范章，渲染后端属框架内部实现。

### 命名空间分层

| 命名空间 | 内容 |
|----------|------|
| `Arc.UI` | 基类根命名空间（`Element`、`Application`、`ResourceDictionary`、绑定/通知原语） |
| `Arc.UI.Components` | 派生组件（`Window`、`Button`、`TextBlock`、`ListView`、`CodeEditor` 等控件） |
| `Arc.UI.Components.Layout` | 布局组件（`StackPanel`、`Grid`、`DockPanel`、`Canvas`、`ScrollView`） |
| `Arc.UI.Rendering` | `IRender`（渲染后端抽象） |
| `Arc.UI.Rendering.Wgpu` | `WgpuRender`（唯一渲染后端） |
| `Arc.UI.Styling` | `Style`、`Setter`、`ThemeDictionary`、`ResourceDictionary` 等样式/资源 |

层级原则：基类放根命名空间，派生实现在子命名空间；子命名空间天然引用穿透父命名空间类型。

### 库结构（解决方案 workspace）

`std/UI/` 是 UI 域**解决方案根**（`[workspace]` 聚合，对标 `std/Web/`），按独立组件库拆分，后续可继续追加。**Arc.UI 研发版图**：

| 目录 | 包 | 内容 |
|------|-----|------|
| `std/UI/Core/` | `Arc.UI` | 核心库：ARML 声明式框架 + wgpu 唯一渲染后端（原 std/UI 全部内容） |
| `std/UI/Edit/` | `Arc.UI.Edit` | 代码编辑组件库：CodeEditor（视口虚拟化 · mmap piece-table）自 Core 迁移至此（含 TextBuffer/LineIndex/EditorViewport/EditorInputRouter） |
| `std/UI/Md/` | `Arc.UI.Md` | Markdown 组件库：Markdown 解析/渲染组件（尚未实现） |
| `std/UI/WebView/` | `Arc.UI.WebView` | 浏览器组件库：系统 WebView 零拷贝捕获进 wgpu 合成面 + 注入脚本/自定义协议桥（设计权威 [webview-surface](../rfc/037-ui/references/webview-surface.md)；尚未实现） |
| `std/UI/WebWindow/` | `Arc.UI.WebWindow` | 效率开发框架（未来规划）：基于 Arc.UI.WebView + Arc.Web 的 Tauri 式装配（尚未实现） |
| `std/UI/Simulator/` | `Arc.UI.Simulator` | 模拟器承载组件库：模拟器界面集成（如 Android 模拟器核心屏幕，纹理表面承载；尚未实现） |

依赖拓扑：`Edit`/`Md`/`Simulator`/`WebView` → `Core`；`WebWindow` → `WebView` + `Core` + `Arc.Web`。`arc build std/UI` 按拓扑序一键构建本域。跨库移动组件时命名空间不变（目录可解耦于命名空间，RFC 037 §2）。

**ARML 消费者 internal 放行**：命名空间索引全 std 扫描，ARML 项目（合并框架模式）构建时会把组件库文件（如 `Edit` 的 `CodeEditor`）并入编译单元；组件库访问归用户包的 Core internal（如 `EditorViewport` → `LayoutHelper`）时，**用户项目 `internals_visible_to` 须同时放行 `Arc.UI` 与所用组件库**（`["Arc.UI", "Arc.UI.Edit"]`，对齐 `examples/ArmlDemo` 先例）。

## 快速开始

一个最小的窗口应用由三部分组成：ARML 标记文件（界面树）、code-behind（逻辑）、以及启动入口。

### 1. 标记文件 `MainWindow.arml`

ARML 采用 WPF xaml 心智模型：根元素声明 `x:Class` 指定配套类，属性经 `{Binding Path}` 绑定到 `DataContext` 模型。

```arml
<Window x:Class="Demo.MainWindow" Title="{Binding Title}">
    <StackPanel vertical="true" Padding="16">
        <TextBlock Text="{Binding Greeting}" />
        <Button Content="点击" Command="{Binding Click}" />
    </StackPanel>
</Window>
```

### 2. code-behind `MainWindow.arml.as`

配套类持有 `DataContext`，可重写生命周期方法 `OnLoaded`/`OnClosed`。

```as
namespace Demo;

using Arc.UI;
using Arc.UI.Components;

public class MainWindow : Window {
    public MainWindow() {
        this.DataContext = new MainViewModel();
    }

    public override void OnLoaded() {
        base.OnLoaded();
        // 初始化完成后逻辑
    }
}
```

### 3. 视图模型

用 `[Observable]` 特性声明属性变更通知，配合 `ICommand` 承载命令：

```as
namespace Demo;

using Arc.UI;

public class MainViewModel {
    [Observable] public string Title;
    [Observable] public string Greeting;
    public ICommand Click;
}
```

`[Observable]` 由编译器合成属性变更通知，绑定自动刷新。集合变更用 `ObservableCollection<T>`。

### 4. 启动入口

```as
using Arc.UI.Components;

void Main() {
    Application app = new Application();
    Window win = new MainWindow();
    win.Show();
    app.Run();
}
```

`Application` 是唯一解析根：内置 Light/Dark 主题默认注册并并入，`Run` 启动帧泵渲染窗口。

## 核心 API

### 窗口与宿主

| 类型 | 说明 |
|------|------|
| `Arc.UI.Components.Window` | 顶级窗口容器；`Title`/`Left`/`Top` 特有依赖属性，继承 `Width`/`Height`/`Content` |
| `Arc.UI.Components.Application` | 应用宿主，唯一解析根；`Run()`/`RunAsync()`，内置主题装配 |
| `Arc.UI.Components.ContentControl` | 内容容器基类，承载 `Content` |
| `Arc.UI.Markup.Element` | 全部元素的基类，`GetValue<T>`/`SetValue<T>` 依赖属性存取 |

### 布局

布局组件位于 `Arc.UI.Components.Layout`：

| 类型 | 行为 |
|------|------|
| `StackPanel` | 线性排布，`vertical="true"` 切换纵向，`Padding`/`Margin` 边距 |
| `Grid` | 网格布局，`RowDefinition`/`ColumnDefinition` 定义行列 |
| `DockPanel` | 停靠布局 |
| `Canvas` | 绝对定位布局 |
| `ScrollView` | 滚动容器 |
| `VirtualizingStackPanel` | 虚拟化列表面板，只物化视口内条目 |

横向/纵向等常量采用强类型枚举（对标 WPF），不写裸字符串。

### 数据绑定与通知

| 机制 | 用途 |
|------|------|
| `{Binding Path}` | 属性路径投影，绑定路径编译期对照模型类型解析，绑错即编译期报错 |
| `[Observable]` 特性 | 属性变更通知，触发绑定刷新 |
| `ObservableCollection<T>` | 集合变更通知，驱动列表刷新 |
| `DataContext` | 每个元素的强类型数据上下文 |

绑定带生命周期管理：弱引用避免长生命周期宿主泄漏，确定性退订保证不悬挂。

### 渲染与虚拟化

| 面 | 说明 |
|----|------|
| 渲染后端 | 唯一 wgpu 后端 `Arc.UI.Rendering.Wgpu.WgpuRender`，实现 `IRender` |
| 渲染循环 | 数据驱动 + 脏区局部更新，避免全树重绘 |
| 虚拟化 | 大数据量列表只物化视口内条目，滚动按需回收/创建 |

### 主题

主题资源分两层：色值经 `std/UI/Core/Themes/{Light,Dark}.arml` 声明（运行时由生成的 `BuiltInThemeColors` 填入字典；设计见 [builtin-theme-resources](../rfc/037-ui/references/builtin-theme-resources.md)），几何/深度/时长经代码定义（`CornerRadius`/`Elevation`/`motion`）。`SwitchTheme` 一条调用全链生效：

```as
SwitchTheme("Dark");   // 或 "Light"
```

内置主题默认引入 `Application` 成为唯一解析根，按 Light/Dark 开放；三方定制经「覆盖键」覆盖，编译期扁平化聚合。

### 自适应布局

自适应以类型安全值元素 + 结构化 `Match` 条件承载：编译期枚举状态空间、运行期投影到目标平台/分辨率。

```arml
<Double x:Key="Spacing.Page">
    <Match Tier="sm" Value="8" />
    <Match Tier="md" Value="16" />
    <Match Tier="lg" Value="24" />
    <Match Value="16" />
</Double>
```

### CodeEditor

`Arc.UI.Components.CodeEditor` 提供代码编辑控件，支持基础样式定制。经显式装配，编辑器可承载多标签页工作区，标签页关闭按钮遵循 VSCode 风格（激活态常显、非激活态悬停显）。

### 显式装配

UI 贡献（菜单、状态栏、标签页）经显式静态注册装配到宿主：宿主接收贡献直接绑定为 `ViewModel`，经数据绑定 + 槽位/模板定制呈现，运行时零反射扫描。菜单/状态栏支持数据绑定 + 槽位/模板设计，实现完全 MVVM 数据驱动的业务-UI 交互。

## 边界

- **语言级能力**：泛型、表达式树、可空与绑定机制见规范章；本分册只讲 UI 框架消费方式。
- **GPU 渲染**：wgpu 后端属框架内部实现，不另立主题。
- **图像与图形 API**（`Arc.Drawing`、二维码、条码）见对应领域文档。
- **显式装配机制**：工具 / DI / Web 端点 / UI 贡献的显式静态注册见规范章 [RFC 037 §6](../rfc/037-ui.md)；本分册只讲 UI 侧装配。

---

上一节：[index.md](index.md) · 下一节：[ai-host.md](ai-host.md)
