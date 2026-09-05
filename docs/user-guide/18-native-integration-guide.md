# 18 Native 组件集成

本章详细说明如何在 Arc 项目中集成第三方 Native 组件——以浏览器引擎（WebView2 / CEF / Miniblink）为典型案例，覆盖从契约声明到窗口嵌入的完整链路。

> **平台现状（1.0）**：本章完整链路以 **Windows** 为交付与验证面（示例库 WebView2/CEF/Miniblink 均为 Windows 组件）。`.ani` 契约、编译期符号验证与动态库机制本身平台中立（命名规则 `.dll`/`.so`/`.dylib` 见 [12 运行时 ABI](12-runtime-abi.md)）；`WindowHost` 原生窗口句柄 API 在 runtime 层为三平台实现（映射见「原生窗口集成」），但 Linux/macOS 的 Arc.UI 渲染链与原生模块 vendor 未随 1.0 交付（wgpu 底座 M3+；平台能力边界见 [11 编译模型](11-compilation-model.md)）——相关示例请在 Windows 上验证。

Arc 提供**两条互补路径**集成 Native 组件，开发者可根据场景选择：

| 路径 | 适用场景 | 核心机制 | 对标 C# |
|------|---------|---------|---------|
| **A. 验证式 FFI 直接集成** | 宿主程序直接调用 C API | `.ani` 契约 + 编译期符号验证 | `DllImport`（但更强） |
| **B. 动态库插件式集成** | 可复用库 / 运行时插件 | `AssemblyLoadContext` + `Entry<T>` | `AssemblyLoadContext` |

两条路径可组合使用——动态库内部通过 `.ani` 调用浏览器引擎 C API，宿主通过 `AssemblyLoadContext` 加载动态库。

## 能力矩阵

以下基础设施构成 Native 集成的完整支撑：

| 能力 | API / 机制 |
|------|-----------|
| C API 契约声明 | `.ani` 文件 + `native module` 语法 |
| 编译期符号验证 | `llvm-nm` / `dumpbin` / `nm` 扫描 |
| 自动 marshal | `string↔const char*` / `List<T>↔T*+size_t` / `struct` / `NativePtr` |
| `[native].ani-native-lib` | 链接器库搜索路径配置 |
| C 源实现编译纳入 | 契约内 `source = "foo.c"` → clang 编译 `.o` 并链接（符号由本地 `.o` 提供） |
| 同目录同名配对回退 | 未声明 `source`/`library` → 契约同目录同名 `.c`/库自动发现 |
| 动态库加载/卸载 | `rt_library_load` / `rt_library_sym` / `rt_library_unload` |
| 包元数据读取 | `rt_library_get_meta` → `Assembly.PackageMeta` |
| `AssemblyLoadContext` | `Load` / `LoadByName` / `LoadFromDirectory` / 探针路径 |
| 泛型 Entry 入口 | `asm.Entry<TResult>()` / `asm.Entry<TP, TR>(args)` |
| 生命周期钩子 | `IAssemblyLifecycle`（OnResolving / OnLoaded / OnUnloading / OnUnloaded） |
| 原生窗口句柄提取 | `WindowHost.NativeHandle` / `WindowHost.CreateWindow` |

---

## 路径 A：验证式 FFI 直接集成

### 原理

通过 `.ani` 契约文件声明外部 C 库的函数签名，编译器在编译期验证符号存在性与类型兼容性，用户侧零 `unsafe`、零原生指针。这是对标 C# `[DllImport]` 的方案，但提供了编译期强保证。

### 第 1 步：编写 `.ani` 契约文件

将 `.ani` 文件放在项目根的 `native/` 目录下，`arc build` 自动扫描发现（编译器内置契约——`libc`/`rt_library` 等——自动扫描，用户项目 `native/` 中的同模块名契约覆盖内置）。

```arc
// native/webview2.ani — WebView2 C API 契约（用户项目契约放项目根 native/）
//
// 本契约声明浏览器引擎的 C API 子集，Arc 侧通过验证式 FFI
// 机制调用——typeck 在编译期验证符号与类型，codegen 直接 emit 调用，
// 用户侧零 unsafe、零原生指针。

native module webview2 {
    // ============================================================
    // 环境与控制器
    // ============================================================

    /// 创建 WebView 环境。
    /// options 传 null 使用默认配置。
    fn wv2_create_environment(NativePtr options) -> NativePtr;

    /// 将 WebView 附着到原生窗口。
    /// parent_window: Windows 为 HWND，macOS 为 NSView*，Linux 为 X11 Window。
    fn wv2_create_controller(NativePtr env, NativePtr parent_window) -> NativePtr;

    /// 销毁控制器，释放资源。
    fn wv2_destroy(NativePtr controller) -> void;

    // ============================================================
    // 导航
    // ============================================================

    /// 导航到 URL。返回 0 成功，非 0 失败。
    fn wv2_navigate(NativePtr controller, string url) -> int;

    /// 获取当前 URL。
    fn wv2_get_url(NativePtr controller) -> string?;

    /// 后退 / 前进 / 刷新。
    fn wv2_go_back(NativePtr controller) -> int;
    fn wv2_go_forward(NativePtr controller) -> int;
    fn wv2_reload(NativePtr controller) -> int;

    // ============================================================
    // JS 互操作
    // ============================================================

    /// 执行 JavaScript 脚本，返回结果字符串（JSON 序列化）。
    fn wv2_execute_script(NativePtr controller, string script) -> string?;

    /// 注册主机对象供 Web 端调用。
    fn wv2_add_host_object(NativePtr controller, string name, NativePtr obj) -> int;

    // ============================================================
    // 事件回调（需 C 函数指针回调机制）
    // ============================================================

    /// 注册导航完成回调。
    /// 注意：回调参数为 C 函数指针，需 lambda marshal 支持。
    fn wv2_set_navigation_completed(NativePtr controller, NativePtr callback) -> int;
}
```

### 第 2 步：配置 `arc.toml`

在 `arc.toml` 中声明 native 库搜索路径：

```toml
[package]
name = "MyBrowserApp"
edition = "1"
version = "0.1.0"
kind = "binary"
namespace = "MyBrowserApp"

[native]
# 链接器库搜索路径——编译期符号验证在此路径下查找 webview2.dll
ani-native-lib = ["vendor/lib", "C:/Program Files/Microsoft/Edge WebView2"]

[dependencies]
# 本地 path 依赖
compiler = { path = "../compiler" }
```

**契约文件发现规则**：编译器内置契约与项目根 `native/` 目录下所有 `.ani` 文件自动扫描，无需在 manifest 中显式声明。

### 第 3 步：在 Arc 代码中调用

```arc
using Arc;
using Arc.Native.Webview2;    // 引入 webview2 契约

void Main() {
    // 1. 创建 WebView 环境
    NativePtr env = webview2.wv2_create_environment(null);
    if (env == null) {
        Console.WriteLine("Failed to create WebView2 environment.");
        return;
    }

    // 2. 获取原生窗口句柄（见下文「窗口集成」小节）
    long hwnd = WindowHost.CreateWindow("My Browser", 1280, 720);
    long nativeHandle = WindowHost.NativeHandle(hwnd);

    // 3. 创建 WebView 控制器并附着到窗口
    NativePtr controller = webview2.wv2_create_controller(env, ptr(nativeHandle));
    if (controller == null) {
        Console.WriteLine("Failed to create WebView2 controller.");
        WindowHost.DestroyWindow(hwnd);
        return;
    }

    // 4. 导航到首页
    webview2.wv2_navigate(controller, "https://arc-lang.dev");

    // 5. 进入消息循环（阻塞直到窗口关闭）
    WindowHost.RunEventLoop(hwnd);

    // 6. 清理
    webview2.wv2_destroy(controller);
    WindowHost.DestroyWindow(hwnd);
}
```

### 编译期符号验证

`arc build` 时编译器自动执行符号验证：

1. **扫描目标库**：在 `[native].ani-native-lib` 路径下用平台工具（Windows: `dumpbin /symbols`，Linux: `nm`，通用: `llvm-nm`）扫描 `webview2.dll` / `libwebview2.so` 的符号表
2. **逐一校验**：契约中声明的每个 `fn` 在目标库中必须存在，签名 ABI 兼容
3. **编译错误**：符号缺失或签名不匹配即编译错误，**编译通过即运行时无链接错误**

```
error: native symbol not found in 'webview2.dll'
  --> native/webview2.ani:15:5
   |
15 |     fn wv2_create_environment(NativePtr options) -> NativePtr;
   |     ^^^^^^^^^^^^^^^^^^^^^^^^ symbol 'wv2_create_environment' not found
```

### Native 源实现：C 源码随项目编译纳入

除链接**已编译**的外部库 / DLL（`library`）外，`.ani` 契约可声明一段**随本项目编译纳入**的 C 源码（`source`）。这使真实用户引入原生能力时**无需改动编译器**——C 源路径在契约内自声明（对标 `library` 声明 DLL 路径、`source` 声明源码路径，单一 `.ani` 协议二选一）。

**契约 + C 源 + 宿主代码**：

```arc
// native/math.ani — `source` 指向相对本 .ani 所在目录的 C 源
native module math {
    source = "math.c";
    fn add_c(int a, int b) -> int;
}
```

```c
/* native/math.c — 提供与契约一致的符号，被 `arc build` 自动编译链接 */
int add_c(int a, int b) { return a + b; }
```

```arc
using Arc;

void Main() {
    Console.WriteLine("add_c(7, 42) = " + math.add_c(7, 42).ToString());
}
```

`arc build` 自动完成：发现 `source` → 用 clang 把 `math.c` 编译为 `.o` → 与其余对象一并链接进产物。该模块符号由本地 `.o` 提供，因此**跳过外部 `-l<name>` 与外部库符号验证**（静态模块亦不依赖 `.so`/`.lib`）。

**同目录同名配对（回退发现）**：当 `.ani` **未声明 `source` 也未声明 `library`** 时，编译器按契约文件所在目录查找同名词源/词库：

1. 存在同名 `.c`（`foo.ani` ⇔ 同目录 `foo.c`）→ 自动当作源实现编译；
2. 否则存在同名词库（如 `foo.dll` / `libfoo.so`）→ 自动从该契约目录链接；
3. 两者皆无 → 维持原设计（全局 `ani-native-lib` 搜索列表 / 系统路径）。

```
native/
├── foo.ani        # 显式 source = "foo.c"（推荐，路径意图明确）
└── foo.c
```

显式声明优先于回退；回退以真实文件存在为准，且不影响 `libc`/`wgpu` 等内置契约（它们无同目录配对文件，行为不变）。

---

## 路径 B：动态库插件式集成

### 原理

将浏览器组件封装为 Arc 动态库（`kind = "library"` + `dynamic = true`），宿主程序通过 `AssemblyLoadContext` 动态加载，通过泛型 `Entry<T>` 入口调用。对标 C# `AssemblyLoadContext.LoadFromAssemblyPath` + 反射调用。

### 第 1 步：编写库项目

**目录结构**：

```
browser-plugin/
├── arc.toml
├── native/
│   └── webview2.ani        # 浏览器引擎 C API 契约
└── BrowserHost.as            # 库实现
```

**`arc.toml`**：

```toml
[package]
name = "browser-plugin"
edition = "1"
version = "1.0.0"
kind = "library"
dynamic = true
namespace = "Arc.Web.Browser"

[native]
ani-native-lib = ["vendor/lib"]
```

**库入口实现**：

```arc
// browser-plugin/BrowserHost.as
namespace Arc.Web.Browser;

using Arc;
using Arc.Runtime;
using Arc.Native.Webview2;

/// 浏览器宿主——封装 WebView2 引擎的生命周期与交互。
public class BrowserHost {
    private NativePtr _env;
    private NativePtr _controller;

    /// 初始化浏览器并附着到指定原生窗口。
    public BrowserHost(long nativeWindowHandle) {
        this._env = webview2.wv2_create_environment(null);
        this._controller = webview2.wv2_create_controller(
            this._env, ptr(nativeWindowHandle));
    }

    /// 导航到 URL。
    public void Navigate(string url) {
        webview2.wv2_navigate(this._controller, url);
    }

    /// 执行 JavaScript 并返回结果。
    public string? ExecuteScript(string js) {
        return webview2.wv2_execute_script(this._controller, js);
    }

    /// 销毁浏览器实例。
    public void Dispose() {
        if (this._controller != null) {
            webview2.wv2_destroy(this._controller);
            this._controller = null;
        }
    }
}

/// 库入口配置——供宿主通过 asm.Entry<BrowserConfig, BrowserHost> 调用。
public struct BrowserConfig {
    public long NativeWindowHandle;
    public string InitialUrl;
}

/// 库入口函数——接收配置，返回初始化后的 BrowserHost 实例。
public BrowserHost CreateBrowserHost(BrowserConfig config) {
    var host = new BrowserHost(config.NativeWindowHandle);
    host.Navigate(config.InitialUrl);
    return host;
}
```

### 第 2 步：编译动态库

```bash
arc build --dynamic
```

产物：`bin/browser-plugin.dll`（Windows）/ `libbrowser-plugin.so`（Linux）/ `libbrowser-plugin.dylib`（macOS）。

编译时自动嵌入 `__arc_package_meta` 全局符号（包含 `name\0version\0edition\0`），供运行时 `Assembly.PackageMeta` 读取。

### 第 3 步：宿主程序动态加载

```arc
using Arc;
using Arc.Runtime;
using Arc.Web.Browser;      // 引入库的公共类型

void Main() {
    var alc = AssemblyLoadContext.Default;

    // 添加插件搜索路径
    alc.AddProbingPath("./plugins");

    // 按名称加载（在 ./plugins 中搜索 browser-plugin.dll）
    Assembly asm = alc.LoadByName("browser-plugin");

    // 读取包元数据（对齐 C# asm.GetName()）
    Console.WriteLine("Loaded: " + asm.FullName);
    // 输出: Loaded: browser-plugin, Version=1.0.0, Edition=1

    // 创建窗口并获取原生句柄
    long hwnd = WindowHost.CreateWindow("My Browser", 1280, 720);
    long nativeHandle = WindowHost.NativeHandle(hwnd);

    // 调用库的泛型 Entry 入口
    var config = new BrowserConfig {
        NativeWindowHandle = nativeHandle,
        InitialUrl = "https://arc-lang.dev"
    };
    BrowserHost host = asm.Entry<BrowserConfig, BrowserHost>(config);

    // 进入消息循环
    WindowHost.RunEventLoop(hwnd);

    // 清理
    host.Dispose();
    WindowHost.DestroyWindow(hwnd);

    // 可选：卸载动态库
    alc.Unload(asm);
}
```

### 生命周期管理

`AssemblyLoadContext` 提供完整生命周期钩子：

```arc
using Arc.Runtime;

/// 自定义生命周期管理器——加载时校验版本，卸载前安全检查。
public class BrowserPluginLifecycle : IAssemblyLifecycle {
    public string? OnResolving(AssemblyResolvingArgs args) {
        // 自定义库发现逻辑（如从远程下载）
        Console.WriteLine("Resolving: " + args.RequestPath);
        return args.RequestPath;  // 返回解析后的路径，null 表示无法解析
    }

    public void OnLoaded(AssemblyLoadArgs args) {
        // 加载完成——校验包版本兼容性
        var meta = args.LoadedAssembly.PackageMeta;
        if (meta.Name != "browser-plugin") {
            throw new InvalidOperationException(
                "Unexpected assembly: " + meta.Name);
        }
        Console.WriteLine("Loaded " + meta.Name + " v" + meta.Version);
    }

    public void OnUnloading(AssemblyUnloadArgs args) {
        // 卸载前检查——确保资源已释放
        Console.WriteLine("Unloading: " + args.UnloadingAssembly.Name);
        // 设置 args.Cancel = true 可阻止卸载
    }

    public void OnUnloaded(AssemblyUnloadedArgs args) {
        Console.WriteLine("Unloaded: " + args.Name);
    }
}

// 注册生命周期管理器
AssemblyLoadContext.Default.Use<BrowserPluginLifecycle>();
```

---

## 原生窗口集成

浏览器引擎需要附着到原生窗口。Arc 通过 `WindowHost` 提供原生窗口句柄提取能力——runtime 层窗口后端按平台实现（下表为各平台句柄映射）。**1.0 已验证并随包交付的面为 Windows**；Linux（X11）/macOS（AppKit）后端已接线但未随 1.0 验收（UI/vendor 面边界见 [11 编译模型](11-compilation-model.md)），示例请以 Windows 为准。

### 平台句柄映射

| 平台 | 原生句柄类型 | `WindowHost.NativeHandle` 返回值 |
|------|------------|-------------------------------|
| Windows | `HWND` | `i64` 承载 `HWND` 指针 |
| Linux | X11 `Window` | `i64` 承载 `Drawable` |
| macOS | `NSView*` | `i64` 承载 `NSView*` 指针 |

### 使用模式

```arc
using Arc.UI.Components;

// 方式 1：直接创建平台窗口（不进入消息循环）
long hwnd = WindowHost.CreateWindow("Browser Window", 1280, 720);
long nativeHandle = WindowHost.NativeHandle(hwnd);
// ... 将 nativeHandle 传给浏览器引擎 ...
WindowHost.RunEventLoop(hwnd);   // 进入消息循环
WindowHost.DestroyWindow(hwnd);  // 销毁窗口

// 方式 2：在 Arc UI Window 的 OnLoaded 中获取
public class BrowserWindow : Window {
    public override void OnLoaded() {
        long hwnd = WindowHost.CreateWindow(this.Title, 1024, 768);
        long nativeHandle = WindowHost.NativeHandle(hwnd);
        // ... 使用 nativeHandle 初始化浏览器 ...
        WindowHost.RunEventLoop(hwnd);
        WindowHost.DestroyWindow(hwnd);
    }
}
```

---

## 典型场景：完整浏览器组件集成

以下是一个完整的浏览器组件集成示例，展示路径 A + 窗口集成的端到端用法。

### 项目结构

```
BrowserApp/
├── arc.toml
├── native/
│   └── webview2.ani        # WebView2 C API 契约
└── Program.as                # 宿主程序入口
```

### `arc.toml`

```toml
[package]
name = "BrowserApp"
edition = "1"
version = "0.1.0"
kind = "binary"
namespace = "BrowserApp"

[native]
ani-native-lib = ["vendor/lib"]
```

### `Program.as`

```arc
using Arc;
using Arc.UI.Components;
using Arc.Native.Webview2;

void Main() {
    // 1. 创建窗口
    long hwnd = WindowHost.CreateWindow("Arc Browser", 1280, 720);
    long nativeHandle = WindowHost.NativeHandle(hwnd);

    // 2. 初始化 WebView2
    NativePtr env = webview2.wv2_create_environment(null);
    NativePtr controller = webview2.wv2_create_controller(env, ptr(nativeHandle));

    // 3. 导航
    webview2.wv2_navigate(controller, "https://arc-lang.dev");

    // 4. 进入消息循环
    WindowHost.RunEventLoop(hwnd);

    // 5. 清理
    webview2.wv2_destroy(controller);
    WindowHost.DestroyWindow(hwnd);
}
```

### 编译与运行

```bash
# 编译（自动验证 webview2.dll 符号）
arc build

# 运行
arc run
# 或直接运行
./bin/BrowserApp.exe
```

如果 `webview2.dll` 中缺少契约声明的符号，编译期即报错，不会到运行时才发现。

---

## 批量插件加载

`AssemblyLoadContext.LoadFromDirectory` 支持从目录批量加载动态库（插件目录场景）：

```arc
using Arc.Runtime;

void Main() {
    var alc = AssemblyLoadContext.Default;

    // 批量加载 ./plugins 目录下所有兼容的动态库
    List<Assembly> plugins = alc.LoadFromDirectory("./plugins");

    Console.WriteLine("Loaded " + plugins.Count.ToString() + " plugins:");
    for (int i = 0; i < plugins.Count; i++) {
        Assembly asm = plugins[i];
        Console.WriteLine("  - " + asm.FullName);

        // 调用每个插件的 Entry 入口
        // var result = asm.Entry<PluginConfig, IPlugin>(config);
    }
}
```

---

## 与 C# 生态对比

| 能力 | C# | Arc | 优势 |
|------|-----|-----|------|
| C API 调用 | `[DllImport]` + `unsafe` | `.ani` 契约 + `using` | 编译期符号验证，零 unsafe |
| 符号缺失发现 | 运行时 `EntryPointNotFoundException` | 编译期错误 | 编译通过即无链接错误 |
| 类型 marshal | 人工 `[MarshalAs]` | 编译器自动 marshal | 无需手写 marshal 代码 |
| 动态库加载 | `AssemblyLoadContext.LoadFromAssemblyPath` | `AssemblyLoadContext.Load` / `LoadByName` | API 一致，探针路径更灵活 |
| 库入口调用 | `Assembly.GetType` + `MethodInfo.Invoke`（反射） | `asm.Entry<TP, TR>(args)`（编译期泛型） | 零反射，零装箱，类型安全 |
| 包元数据 | `AssemblyName` + `asm.GetName()` | `asm.PackageMeta` + `asm.FullName` | 概念对齐，懒加载 |
| 生命周期管理 | 无原生支持 | `IAssemblyLifecycle` 四钩子 | 加载/卸载全程可定制 |
| 原生窗口嵌入 | `Control.Handle` / `HwndSource` | `WindowHost.NativeHandle` | 跨平台统一 API |
| 批量加载 | 需手动遍历目录 | `LoadFromDirectory` | 一行代码完成 |

### 开发者体验对照

**C# WebView2 集成**：

```csharp
// 需要 NuGet 包 + COM Interop + 运行时初始化
var webView2 = new WebView2();
await webView2.EnsureCoreWebView2Async();
webView2.Source = new Uri("https://arc-lang.dev");
```

**Arc WebView2 集成**：

```arc
// .ani 契约 + 编译期验证 + 直接调用
NativePtr controller = webview2.wv2_create_controller(env, ptr(nativeHandle));
webview2.wv2_navigate(controller, "https://arc-lang.dev");
```

开发者做法与 C# **基本一致**——引入依赖、调用方法、嵌入窗口。Arc 的优势在于编译期保证更强（符号验证 + 自动 marshal）。

---

## 最佳实践

### 1. 契约粒度控制

`.ani` 契约应只声明实际使用的 C API 子集，不要全量翻译头文件：

```arc
// ✅ 好——只声明需要的函数
native module webview2 {
    fn wv2_create_environment(NativePtr options) -> NativePtr;
    fn wv2_create_controller(NativePtr env, NativePtr hwnd) -> NativePtr;
    fn wv2_navigate(NativePtr controller, string url) -> int;
    fn wv2_destroy(NativePtr controller) -> void;
}

// ❌ 差——全量翻译 200+ 函数，维护成本高
native module webview2 {
    fn wv2_create_environment(...) -> ...;
    fn wv2_create_controller(...) -> ...;
    // ... 200 个不需要的函数 ...
}
```

### 2. 动态库资源管理

使用 `using` 语句或 `Dispose` 模式确保动态库句柄释放：

```arc
// AssemblyLoadContext.UnloadAll() 会在程序退出时统一释放
// 但建议显式管理关键资源的生命周期
```

### 3. 探针路径组织

将插件动态库放在固定的 `./plugins` 目录下，通过 `AddProbingPath` 统一管理：

```arc
var alc = AssemblyLoadContext.Default;
alc.AddProbingPath("./plugins");
alc.AddProbingPath("./plugins/native");  // native 依赖库
```

### 4. 版本兼容性校验

通过 `IAssemblyLifecycle.OnLoaded` 钩子校验加载的库版本：

```arc
public void OnLoaded(AssemblyLoadArgs args) {
    var meta = args.LoadedAssembly.PackageMeta;
    if (meta.Version != "1.0.0") {
        throw new InvalidOperationException(
            "Version mismatch: expected 1.0.0, got " + meta.Version);
    }
}
```

### 5. 错误处理

Native 调用返回值应统一检查：

```arc
NativePtr controller = webview2.wv2_create_controller(env, ptr(nativeHandle));
if (controller == null) {
    Console.PrintError("Failed to create WebView2 controller.");
    WindowHost.DestroyWindow(hwnd);
    return;
}
```

---

## 关联文档

| 主题 | 文档 |
|------|------|
| `arc.toml` 完整 schema | [17 arc.toml 项目清单参考](17-arc-toml-reference.md) |
| 能力系统 | [15 能力系统](15-capability-system.md) |
| 现有 `.ani` 契约示例 | `crates/arc/native/wgpu-native.ani` / `libc.ani` / `rt_library.ani` |
| `AssemblyLoadContext` 实现 | `std/Arc/Runtime/AssemblyLoadContext.as` |
| `Assembly` 实现 | `std/Arc/Runtime/Assembly.as` |
| `IAssemblyLifecycle` 实现 | `std/Arc/Runtime/IAssemblyLifecycle.as` |
| `WindowHost` 实现 | `std/UI/Core/Components/WindowHost.as` |

---

上一节：[17 arc.toml 项目清单参考](17-arc-toml-reference.md) · 下一节：[附录 A 术语表](appendix-glossary.md)