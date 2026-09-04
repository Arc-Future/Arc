# Arc.UI.WebView：系统 WebView 零拷贝集成（IBrowserSurface 渲染面 + IBrowserBridge 桥）

> 本文是 实现规划）；实现进度亦不在此维护。
>
> 一主题一文档：本文只讲「系统 WebView 集成」。渲染原语与纹理生命周期契约见 [texture-surface](texture-surface.md)；能力系统见 [15 能力系统](../../../user-guide/15-capability-system.md)；编译期显式装配见 [037 §6(../../037-ui.md)；`.ani` 验证式 FFI 见 [016 验证式 FFI 与 Native 加载](../../016-verified-ffi.md)。

## 1. 定位

`Arc.UI.WebView` 让**系统自带浏览器引擎**成为应用 UI 的一等载体：引擎离屏渲染输出经 **GPU 零拷贝捕获**（wgpu-scry 方案）进入 wgpu 合成面，与 Arc.UI 控件处于**同一渲染帧、同一合成器**；前端（HTML/CSS/JS）经**注入脚本 + 自定义协议**的受控桥调用 Arc 原生后端（File/DB/AI/Net/DI）。

| 面 | 正道 | 拒绝 |
|----|------|------|
| 引擎来源 | 系统自带引擎（Windows: Evergreen WebView2；macOS: WKWebView；Linux: WebKitGTK 系统包），**零打包** | 打包 Chromium/CEF；内置自研 HTML/CSS 引擎作为主路径 |
| 渲染集成 | GPU 零拷贝捕获 → wgpu 外部纹理导入 → `DrawTexture` 合成 | 子视图嵌入作为主路径（airspace 族）；CPU 像素读回作为主路径 |
| 前后端桥 | 注入脚本 `__ARC_INTERNALS__` + 自定义协议（`asset://` / `ipc://`）+ 编译期 `[WebCommand]` 命令表 + 能力门闩 | 运行时反射发现；裸 socket/自建 HTTP 服务作为主通道 |
| 组件 | `Arc.UI.Components.WebBrowser`（`VideoSurface` 同构） | 平台原生控件直挂（子 HWND/NSView 直用） |

**命名空间分层**（对齐 [037 §2(../../037-ui.md) 层级原则：基类在根、派生实现在子）：

| 命名空间 | 内容 |
|----------|------|
| `Arc.UI.Components` | `WebBrowser` 组件（公共消费面，`VideoSurface` 兄弟组件） |
| `Arc.UI.WebView` | 宿主与桥：`IBrowserSurface` / `IBrowserBridge` / `WebViewHost` / 协议处理器 / 注入脚本 / `[WebCommand]` 聚合 |

## 2. 背景与动机

- 应用需要 **web 前端生态**（HTML/CSS/JS、React/Vue 类、既有 Web 资产）作为 UI，同时保留 Arc 原生后端能力——这是「桌面 AOT + 系统引擎」组合的根本诉求。
- 三条候选路径的取舍：

| 候选 | 思路 | 判定 |
|------|------|------|
| A. 子视图嵌入（Tauri 默认形态） | webview 铺满窗口，OS 合成 | **不取为主路径**：窗口内出现第二个合成器，违反 arc-ui.mdc「渲染唯一 wgpu」；无法裁剪/变换/混合；输入权威分裂（R3 三原则被破坏）。macOS 亦走 IOSurface 零拷贝，无需子视图 |
| B. 内置精简渲染引擎（Blitz 类） | 自研 wgpu HTML/CSS 渲染器，无 JS | **不取为主路径**：无 JS 引擎则 web 应用不可运行；与 ARML 声明式框架功能重叠（第二套标记，违反单一惯用法）；浏览器团队级工程量。记远期后备（§7 边界） |
| C. **GPU 零拷贝捕获（wgpu-scry 方案）** | 系统引擎离屏合成 → GPU 纹理句柄直通 → wgpu 外部纹理导入 → 与 Arc.UI 同一合成帧 | **采纳**：引擎零打包；渲染唯一 wgpu（外部纹理导入是 wgpu-native 能力，非第二渲染轨）；无 CPU 拷贝、无渲染 IPC；内容可裁剪/变换/混合；AI 原生回读可捕获。**三平台（Windows DXGI / Linux dma-buf / macOS IOSurface）均有成熟实现**（参照 [wgpu-scry](https://github.com/merely-made/wgpu-scry)：producer + native frame 同步 `sync_dx12`/`sync_metal`/`sync_vulkan` + [parity-matrix](https://github.com/merely-made/wgpu-scry/blob/main/docs/parity-matrix.md) 实证基线） |

- 仓库已具备落点：纹理表面管线（`DrawTexture` + 多槽动态纹理注册表 + `VideoSurface` 组件，见 [texture-surface](texture-surface.md)）——WebView 是这条管线的**外部纹理变体**消费方，不新开渲染轨。

## 3. 设计决策

### 3.1 双面架构：IBrowserSurface ⊕ IBrowserBridge

渲染与通信是**两条正交轴**，分别抽象，组件不感知平台：

```as
namespace Arc.UI.WebView;

/// <summary>渲染面——引擎 GPU 帧输出到 wgpu 纹理 + 输入注入。每平台一实现。</summary>
public interface IBrowserSurface {
    /// <summary>下一帧 GPU 句柄（Windows: DXGI 共享纹理；Linux: dmabuf fd；macOS: IOSurface）。</summary>
    NativePtr NextFrameHandle();
    /// <summary>新帧已就绪信号（引擎 GPU 侧完成 → UI 线程处理导入 + 重绘）。</summary>
    Signal<bool> FrameArrived;
    /// <summary>输入注入（坐标已换算为 guest CSS 像素）。</summary>
    void InjectPointer(int type, double x, double y, int button, int mods);
    void InjectWheel(double dx, double dy);
    void InjectKey(int virtualKey, int mods, int down);
    void InjectText(string utf8);
}

/// <summary>桥——导航/JS 求值/协议注册/脚本注入/事件。每平台一实现（同一引擎宿主）。</summary>
public interface IBrowserBridge {
    void Navigate(string url);
    void GoBack();
    void GoForward();
    void Reload();
    Task<string?> ExecuteScriptAsync(string script, CancellationToken ct);
    void RegisterProtocolHandler(string scheme, Action<NativePtr request> handler);
    void InjectScript(string source);            // 页面加载时注入
    Signal<bool> NavigationCompleted;
    Signal<string> TitleChanged;
}
```

- **引擎宿主**（每平台薄封装，经 `.ani` 契约调用）同时实现两个接口——渲染面与桥共享同一引擎实例；
- **`WebViewHost`**（`Arc.UI.WebView`）做装配：引擎实例 + 协议注册（`asset://` / `ipc://`）+ 注入脚本 + `[WebCommand]` 命令表 + 能力门闩 + 事件桥。`Application` 级单例（对标引擎环境进程级单例）；
- **`WebBrowser` 组件**只消费接口，不感知平台。

### 3.2 渲染面：GPU 零拷贝捕获（wgpu-scry 方案）

#### 3.2.1 平台机制

| 平台 | 引擎 | 零拷贝路径 |
|------|------|-----------|
| Windows | WebView2（Evergreen 运行时，系统安装） | `ICoreWebView2CompositionController`（官方非窗口模式）→ DComp visual 挂入自有合成树 → 渲染目标为**共享 D3D11 纹理（DXGI）** → wgpu（DX12）`OpenSharedHandle` 导入为 `wgpu::Texture`，经 `sync_dx12` 帧同步 |
| Linux | WebKitGTK（webkit2gtk 系统包） | WebKit 渲染输出 **DMA-BUF** → wgpu（Vulkan）`VK_EXT_external_memory_dma_buf` 外部内存导入 |
| macOS | WKWebView（WebKit 系统框架） | WKWebView 渲染目标 **IOSurface** → wgpu（Metal）IOSurface 外部纹理导入，经 `sync_metal` 帧同步（对齐 WebGPU `importExternalTexture` 提案的 IOSurface 路径） |

- **零拷贝成立的前提**：同一 GPU 上句柄传递，帧不落地 CPU、不走 IPC；
- **成熟度**：三平台路径均为 wgpu-scry 已实证机制（producer + `native_frame/sync_*.rs` 同步层 + [parity-matrix](https://github.com/merely-made/wgpu-scry/blob/main/docs/parity-matrix.md) 对齐矩阵）——本方案沿用同一机制矩阵；**自家验收前不宣称**（宣称纪律，对齐 [036 成熟度](../../036-maturity.md)）；
- **应急降级**：CPU 快照/读回仅作平台面异常时的应急通道，非主路径。

#### 3.2.2 `IRender` 契约扩展：ImportExternalTexture

`IRender` 增一项**外部纹理导入**变体（既有纹理注册表共存，`DrawTexture` 采样路径完全复用）：

```as
/// <summary>导入外部 GPU 纹理（wgpu-scry 方案）。句柄随平台：
/// Windows: DXGI 共享纹理指针；Linux: dmabuf fd；macOS: IOSurface；返回值纹理 id（0 失败）。
/// 此后帧内容由引擎侧更新，本槽位只读采样；尺寸变更须重建导入。</summary>
int ImportExternalTexture(NativePtr gpuHandle, int width, int height);
```

- 渲染端（`WgpuRender`）仅需新增导入实现（`.ani` 面新增 `wgpu_texture_import_external`），**`DrawTexture` / bind group / pipeline p==3 全部复用**（见 [texture-surface](texture-surface.md) §3.1–3.2）；
- 纹理注册表槽位语义扩展：`CreateTexture`（CPU 上传）与 `ImportExternalTexture`（GPU 直通）两类槽，生命周期管理统一（`DestroyTexture` 共用）。

#### 3.2.3 帧同步与生命周期

- **帧就绪**：引擎 GPU 侧完成 → `FrameArrived` 信号 → `UiDispatcher.Post` → 组件在下一帧 `BeginFrame` 前完成导入/同步并 `FramePump.Invalidate()`；
- **同步**：跨 API 同步按平台 **native frame 同步层**落地（对齐 wgpu-scry `native_frame/sync_*.rs`：DX12 fence / Vulkan semaphore / Metal 同步）——fence 级精确同步为设计内建面，非折衷；
- **生命周期**：纹理归组件——`Attach` 导入/创建、`Detach`/卸载销毁（`RegisterDetach` 卸载退订，照抄 [VideoSurface](../../../../std/UI/Core/Components/VideoSurface.as) 已验证模式）；引擎归系统（组件不 Dispose 引擎，宿主级生命周期）；
- **尺寸变更**：浏览器视口 resize → 重建导入 + 通知引擎 `SetBounds`，与 `WgpuRender.Resize` 节奏一致；
- **持续刷新**：页面播放视频/动画时走持续渲染路径（复用 `MotionEngine.Active()` 先例，对齐 `VideoSurface`「持续刷新」）；静止页面回到按需渲染。

### 3.3 桥：注入脚本 + 自定义协议（Tauri 模型）

#### 3.3.1 注入脚本

引擎加载页面时注入一段 JS（WebView2 `AddScriptToExecuteOnDocumentCreated` / WKWebView `WKUserScript` / WebKitGTK 页面脚本），定义 `window.__ARC_INTERNALS__`——前端调用 Arc 后端的**唯一通道**：

- `invoke(cmd, args) → Promise`：命令调用（见 §3.3.3）；
- `emit`/`listen`：事件订阅（见 §3.3.5）；
- 版本/能力探测：`isAvailable`、能力白名单查询。

**无需向 webview 打包任何 JS 运行时**——通道全部寄生在注入脚本上（对齐 Tauri `__TAURI_INTERNALS__` 机制）。

#### 3.3.2 自定义协议

| scheme | 用途 | 语义 |
|--------|------|------|
| `asset://` | 应用资产供给 | 处理器从**编译期嵌入的资产映射**按路径返回字节；webview 视作正常站点，JS 可自由 fetch（`file://` 无此能力） |
| `ipc://` | 命令调用通道 | `invoke` 经 `fetch('ipc://localhost/…')` 发出；处理器按 `cmd` 查编译期命令表，反序列化参数，执行（可 async），结果返回；支持二进制载荷 |

- 协议处理器在引擎层注册（WebView2 `AddWebResourceRequestedFilter` / WKWebView `WKURLSchemeHandler` / WebKitGTK `register_uri_scheme`），经 `.ani` 契约面暴露；
- 开发态资产可切 dev server（对齐既有工具链先例），生产态恒走 `asset://`。

#### 3.3.3 `[WebCommand]` 编译期装配

命令表由**编译期显式装配**生成（对齐 [037 §6(../../037-ui.md) 与 `[AITool]` 同构机制），运行时零反射扫描：

```as
[WebCommand("fs.readText")]
public static Task<string> ReadTextAsync(string path, CancellationToken ct);

[WebCommand("db.query", Capabilities = "db:read")]
public static Task<List<Row>> QueryAsync(string sql, CancellationToken ct);
```

- 编译器聚合生成 `__RegisterWebCommands()` 引导，`WebViewHost` 装配时挂入命令表；
- **禁止**运行时程序集扫描 / 反射发现（单一惯用法，对齐 037 §6 拒绝项）。

#### 3.3.4 异步与序列化

- 命令签名强制 `Async` 后缀 + `CancellationToken`（[arc-language](../../../user-guide/03-encoding-standard.md) 契约 #4）；
- 载荷序列化走既有序列化家族（[022 异步任务与 LINQ/序列化](../../022-async-linq-serialization.md)：JSON 默认，二进制载荷用 [030 Protobuf](../../030-protobuf.md) 或裸字节通道）；
- 引擎侧 JS 求值回调（`ExecuteScriptAsync` 结果）→ `UiDispatcher.Post` 上 UI 线程兑现，**禁止回调线程触碰 UI 对象**。

#### 3.3.5 事件与 Channel

- `WebViewHost.Emit("event", payload)` → 注入脚本分发给 `listen()` 监听器（后端 → 前端推送）；
- **Channel**：事件的高带宽变体，Arc 侧持有句柄多次 `send` 二进制数据（下载进度、AI token 流等），对齐 [038 AI 宿主](../../038-ai-host.md) 流式契约的消费面。

#### 3.3.6 能力门闩

- 每个 `[WebCommand]` 声明所需能力集；运行时在 **IPC 层**按窗口/WebView 强制执行（对齐 [15 能力系统](../../../user-guide/15-capability-system.md) 门闩语义）；
- **页面默认零权限**——未声明的命令一律拒绝，拒绝须显式告警（P3 无静默丢弃，对齐 [037 §8(../../037-ui.md)）；
- 未授权命令不可被 `invoke` 命中，也不暴露于注入脚本的能力探测面。

### 3.4 WebBrowser 组件

命名空间 `Arc.UI.Components`，派生自 `Control`（对齐 `VideoSurface` 归 `Control` 层的既有决定）：

```as
public class WebBrowser : Control {
    // 静态依赖属性
    public static DependencyProperty<string> UrlProperty;          // 导航地址
    public static DependencyProperty<string> TitleProperty;        // 页面标题（只读镜像）
    public static DependencyProperty<bool> CanGoBackProperty;      // 只读镜像
    public static DependencyProperty<bool> CanGoForwardProperty;   // 只读镜像
    public static DependencyProperty<bool> IsLoadingProperty;      // 只读镜像

    // 生命周期（VideoSurface 同构：纹理归组件，引擎归宿主）
    public int Attach(IRender backend, IBrowserSurface surface, int width, int height);
    public void Detach(IRender backend);

    // 导航与互操作（Async 后缀 + CancellationToken）
    public Task NavigateAsync(string url, CancellationToken ct);
    public Task GoBackAsync(CancellationToken ct);
    public Task GoForwardAsync(CancellationToken ct);
    public Task ReloadAsync(CancellationToken ct);
    public Task<string?> ExecuteScriptAsync(string script, CancellationToken ct);

    // 帧就绪（桥回调 → UiDispatcher.Post → 导入 + Invalidate）
    internal void OnFrameArrived(IBrowserSurface surface);
}
```

**ARML 消费**（声明式 + 编译期绑定校验）：

```arml
<WebBrowser Url="{Binding HomeUrl}" />
<Button Content="后退" Command="{Binding BackCommand}" IsEnabled="{Binding CanGoBack}" />
```

- `Url` 变更 → `NavigateAsync`（防重入：进行中导航合并或显式取消）；
- 只读镜像属性经 `[Observable]`/`Signal` 由桥回调在 UI 线程更新，驱动工具栏/地址栏（chrome 是 Arc 的，内容才是 guest 的）；
- 浏览器 chrome（地址栏/工具栏/标签页）用 Arc 组件 + 主题 token 构建，参与既有显式装配（037 §6 标签页/状态栏模式）。

### 3.5 输入注入（R3 三原则）

WebBrowser 是 Arc 焦点域内的**复合焦点停靠点**：自身 `Focusable=true` 的 Tab 停靠项，获得焦点后把键盘/指针/文本经标准注入管线转发给 guest 文档；**禁止引擎直接挂平台消息钩子**（那是子视图方案的输入分裂根源）。

| Arc 事件 | 注入路径 |
|----------|---------|
| 指针 | `PointerRouter` 命中 `WebBrowser` → 坐标换算（DIP → guest CSS px，含 `dpiScale`）→ `InjectPointer` |
| 滚轮 | `ScrollRouter` → `InjectWheel` |
| 键盘 | `FocusManager` → `WebBrowser.OnKeyDown`（[InputElement](../../../../std/UI/Core/Markup/InputElement.as) 虚方法）→ `InjectKey` |
| 文本/IME | `WebBrowser.OnGotFocus` → `ImeBridge` 关联 → `InjectText` |

平台层仍只做机械转换（P2 单一键盘通道，[037 §8(../../037-ui.md)）；`FocusManager` 动态容量为其前置，未落地前不宣称生产级。

### 3.6 线程与生命周期纪律

| 面 | 契约 |
|----|------|
| 回调线程 | 引擎回调（导航完成/标题/帧就绪/JS 结果）一律 `UiDispatcher.Post` 上 UI 线程；回调线程禁触碰任何 UI 对象（[037 §7(../../037-ui.md)） |
| 上传/导入时机 | 必须在 command encoder 创建之前（对齐 [texture-surface](texture-surface.md) §3.4 与 `wgpu_font_atlas_flush` 先例） |
| 可用性 | 引擎缺失（WebKitGTK 未装 / Evergreen 缺失）→ `IsAvailable=false` 诚实失败（对齐 `ani_auto_module_e2e` graceful degradation 先例），禁假装成功 |
| 引擎所有权 | 引擎环境进程级单例，`Application` 装配；组件与库不拥有引擎进程，`Dispose` 只解绑视图/销毁纹理 |

### 3.7 资产供给

- 前端构建产物（`dist/`）**编译期嵌入**二进制（复用编译期资源嵌入机制），经 `asset://` 协议供给；
- 相对路径解析与字体注册同约定（相对 app/project base；构建保证 `bin/<config>/` 下可见或注册诚实失败，见 [custom-fonts](custom-fonts.md) §4）。

## 4. 单一惯用法与架构红线

- **渲染唯一 wgpu**：`ImportExternalTexture` 是 wgpu-native 能力（`rt_wgpu_native` 新增导入面），采样走既有 `DrawTexture` 管线——**不是第二渲染轨**，不新增子视图嵌入主路径；三平台外部纹理（DXGI / dma-buf / IOSurface）均经 wgpu 导入进合成面（CPU 上传仅作应急降级）。arc-ui.mdc「一切上屏渲染经由 `WgpuRender`」字面与精神均满足，**无需例外条款**；
- **与 AI 原生预览后端划界**：037 §10.3 拒绝的「HTML/WebView 预览后端」指**用 webview 充当 Arc.UI 自身预览/评审的渲染后端**（G1/G2 双渲染面）——本方案不改变该拒绝项；`WebBrowser` 承载的是**产品面 web 内容**，且因输出为 wgpu 纹理，天然可被 AL-P0 渲染回读捕获（G3 达成，见 [ai-native-render-capture](ai-native-render-capture.md)）；
- **编译器核心零领域能力**：宿主与桥全在 `std/UI/WebView`（`Arc.UI.WebView`）+ `.ani` 契约，编译器仅提供既有通用机制（FFI/能力系统），零核心 crate 改动；
- **不重复文档**：纹理原语与生命周期见 texture-surface.md；能力系统见 RFC 015；显式装配见 037 §6；序列化见 022/030。

## 5. 平台天花板（诚实表）

| 平台 | 引擎 | 零拷贝路径 | 输入 | 天花板与处置 |
|------|------|-----------|------|-------------|
| Windows | WebView2（Evergreen） | DXGI 共享纹理 → DX12 导入（`sync_dx12`） | 标准注入 | CompositionController 为官方非窗口模式；共享纹理 handoff + fence 同步为专项难点（wgpu-scry 同款，已实证） |
| Linux | WebKitGTK | DMA-BUF → Vulkan 外部内存导入 | 标准注入 | WebKitGTK DMA-BUF 输出面须版本探测；实验面风险存在时走应急 CPU 读回（非主路径） |
| macOS | WKWebView | IOSurface → Metal 导入（`sync_metal`） | 标准注入 | IOSurface 出口贴近 WebKit/Metal 内部面，按 wgpu-scry [parity-matrix](https://github.com/merely-made/wgpu-scry/blob/main/docs/parity-matrix.md) 实证基线对齐；CPU 快照仅作应急降级 |

**宣称纪律**（对齐 [036](../../036-maturity.md)）：wgpu-scry 三平台支持已成熟深入（producer + native frame 同步 + parity-matrix + demo-* 实证）；本方案沿用同一机制矩阵，但**未经 Arc 侧对应平台 e2e 验收不得宣称该平台零拷贝**；三平台能力差异以本表为准。

## 6. 边界（不在此篇）

- **音频**：Arc.UI 无音频管线，浏览器音频由引擎原生处理，本组件不承担；
- **弹窗/文件选择/鉴权/下载**：引擎回调 → `UiDispatcher.Post` → Arc 组件呈现（后续能力）；
- **自研 HTML/CSS 渲染引擎（Blitz 类方案）**：远期后备，仅在「渲染静态 Web 资产且不想启动引擎进程」需求实证后评估；现不立项；
- **多窗口/多 WebView 的引擎环境策略**：进程级单例共享与隔离策略（后续能力）；
- **输入坐标变换矩阵细节**（hit-test 边界、缩放模式下的坐标映射，组件实现子项）。

---

[返回 037 主题入口(../../037-ui.md) · [返回 references 索引](index.md)
