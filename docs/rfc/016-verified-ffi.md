# RFC 016 验证式 FFI 与 Native 加载

## 背景

Arc 以**验证式跨语言互操作**取代传统 `extern "C"`/`[DllImport]`/`unsafe` 模式（信条：安全在编译期完成）：声明式契约文件 `.ani` + 编译期符号验证 + 编译器自动 marshal，用户侧**零 `unsafe`、零原生指针、零 `IntPtr`**。`unsafe` 用户面永拒。

## 设计决策

### `.ani` 契约（Arc Native Interface）

契约扩展名 `.ani`，描述外部 C 库接口。编译器解析为 `NativeModule` AST，typeck 注册为 `StaticClass` 复用 OOP 静态方法分派，codegen 直接发射 `call @<symbol>` 并注入链接标志。

```ani
native module gpu {
    capability gpu;
    load = "auto";                                       // static | runtime | auto（缺省 = static）
    library = Environment.GetEnvironmentVariable("ARC_GPU_LIB");  // 环境变量表达式形态
    // library = "vendor/gpu/lib";                        // 相对执行程序根目录路径形态
    fn gpu_init() -> int;
    fn gpu_shutdown() -> int;
}
```

### 编译期符号验证

`verify_symbols.rs` 用 `llvm-nm`/`dumpbin` 扫描契约声明符号；符号缺失即编译/链接错误（static 路径）。工具不可用或库不可定位时按**尽力验证**降级（warning），不引入新硬失败。

### 自动 marshal

| 类型 | marshal 规则 |
|------|-------------|
| 基元类型 | 直接 C 标量 |
| `string` / `string?` / `void` | `const char*` / `const char*`（null 语义）/ void |
| `List<T>` | `T* + size_t`（`rt_list_buffer_and_size` 零拷贝） |
| 契约 struct | 按 C 布局 |
| `void*` ↔ `object` | FFI 边界装箱（`Expr::Box`/`Unbox` + `rt_box_*` ABI） |
| `NativePtr` | 不透明句柄直传 |
| stdcall | 支持（Windows） |

**装箱点自动插入**（typeck）：仅在 FFI `extern` 函数 `void*`（`object`）形参/返回值处自动插入 `Expr::Box`/`Expr::Unbox`；通用赋值/参数/返回值装箱不引入。装箱 ABI 见 [014 运行时 ABI](014-runtime-abi.md)。

类型白名单制：基元 / string / `List<T>` / 契约 struct / `NativePtr`。

### C 回调

- **无捕获回调**：零开销 trampoline（`native callback` 契约语法）。
- **有捕获回调**：TLS 回调表（`rt_ffi_set/get/clear_callback`），支持 int/string/class 捕获与嵌套回调。
- Arc closure → C 回调经 `rt_cts_callback_trampoline` 转发（`CancellationToken.Register` 用）。

### 能力 gating（capability）

`.ani` 内 `capability <cap>` 声明模块所需能力；绑定能力系统（见能力系统规范）。能力声明是编译期要求，运行时经 `Native.IsAvailable` 查询。

### 统一加载模型（`load` + `library`）

`.ani` 仍是**唯一声明面**（无双轨 API）；`load` 为模块级可选声明，缺省 `"static"` 对既有契约零行为变更。

| 策略 | 编译期 | 链接 | 运行时 |
|------|--------|------|--------|
| `static`（默认） | `-L<dir> -l<name>` + 符号验证 | 直接 `call @sym` | 无 |
| `runtime` | 跳过 `-l`/直接 call；尽力符号验证 | 不链接该库 | **首用懒加载**：`library` 求值 → `rt_library_load` → 逐符号 `rt_library_sym` → per-module 函数指针表 → 间接调用（~2–5ns/次） |
| `auto` | 搜索链可定位 → 等价 static；不可定位 → 降级 runtime | 同 static 或跳过 | 仅降级模块运行时解析 |

**`library` 两形态**（路径解析唯一主机制，无多层路径托底）：

1. **相对路径形态**：相对**执行程序根目录**解析。
2. **环境变量表达式形态**：`Environment.GetEnvironmentVariable("...")`（编译器只识别**固定形态**：接收者 `Environment` 静态类 + 方法 `GetEnvironmentVariable(string)→string` + 单个字符串字面量参数；typeck 做 registry 级强类型校验）；运行时求值为绝对路径（或相对执行程序根目录的相对路径）；**未设置 → Arc 语义返回空串** → 模块优雅降级。

**链接期搜索链**（static/auto 编译期定位）：per-module `library` → `ani-native-lib` 列表（始终以主程序根目录为隐式第一项）→ vendor 注入 → 系统路径（`-L` 在 `-l` 前注入）。该链仅作用于 static/auto 编译期定位，**不作为运行时多级回退链**。

**库文件解析**按平台命名约定：Windows MSVC `<module>.lib`；MinGW `lib<module>.dll.a`/`lib<module>.a`；Linux `lib<module>.so`/`.a`；macOS `lib<module>.dylib`/`.a`。

### C 源实现（`source`）与同目录同名配对回退

`.ani` 仍是**唯一声明面**。除链接**已编译**外部库/`DLL`（`library`）外，模块可声明一段**随本项目编译纳入**的 C 源码（`source`）：

```ani
native module foo {
    source = "src/foo.c";   // 编译器经此发现 C 源 → clang 编译 `.o` → 链接进产物
    fn ping() -> int;
}
```

- **路径基准 = 该 `.ani` 契约文件所在目录**（区别于 `library` 相对执行程序根目录：`source` 是编译期输入而非部署期库目录）。
- 编译器把该 C 源用 clang 编译为 `.o`，与其余对象一并链接（注入 `-I<C 源父目录>`，可 `#include` 同目录头文件）；该模块符号由本地 `.o` 提供 → **跳过外部 `-l<name>` 与外部库符号验证**（静态模块亦不依赖 `.so`/`.lib`）。
- 与 `library`（**已编译库/DLL**）**二选一**：`source` 声明源码接缝，`library` 声明产物接缝。真实用户引入原生能力**无需改动编译器**——编译器只认 `.ani` 契约，C 源路径由契约自声明（`libc`/`wgpu` 等内置契约不受影响）。

**同目录同名配对（回退发现，当不指定位置时的处理规则）**：

`.ani` 未声明 `source` 也**未**声明 `library` 时，按契约文件所在目录查找同名词源/词库：

1. 存在同名 `.c`（`foo.ani` ⇔ 同目录 `foo.c`）→ 回退为**源实现**（等价 `source = "foo.c"`）。
2. 否则存在同名平台库变体（`foo.dll` / `libfoo.so` / `libfoo.dylib` / `foo.lib` / `libfoo.a` 等）→ 回退为**从该契约目录链接**（等价 `library` 填契约目录，作 `-L` 与运行时候选）。
3. 全无配对 → 保持原设计（全局 `ani-native-lib` 搜索列表 / 系统路径）。

显式声明（`source`/`library`）优先于回退；回退以真实文件存在为准，不依赖编译目标平台。

### 失败 / 优雅降级契约

| 模式 | 失败表现 | 降级语义 |
|------|---------|---------|
| `static` | 链接期符号验证/链接失败 = 构建错误 | 现状不变 |
| `runtime` | 解析失败 ≠ 编译失败；尽力符号验证 | `Native.IsAvailable == false`；调用点 `if (Native.IsAvailable(...))` 优雅降级；未加载/失败模块调用 → 抛 `NativeLibraryNotFoundException`（派生 `IOException`，显式可捕获） |
| `auto` | 可定位 → static；不可定位 → 降级 runtime | 同 runtime 降级语义 |

**判定语义**：

- `Native.IsAvailable(name)`：一等查询 API，查 per-module 注册表（复用 `rt_library` 代数/状态机）返回解析/绑定状态。
- 调用未加载/失败模块函数 → 抛 `NativeLibraryNotFoundException`。
- 优雅降级在**调用点用标准语言表达**（`if (Native.IsAvailable(...))`）；**禁止静默 stub**——库缺失不静默，`IsAvailable` 可查询、异常显式可捕获。

**环境变量命名惯例**：`<MODULE>_LIB` / `<MODULE>_PATH`，统一 `ARC_` 前缀（如 `ARC_GPU_LIB` / `ARC_GGML_LIB`）。

### 与动态库加载的关系

`.ani` 管 C 库契约；`AssemblyLoadContext` 管 Arc 动态库 Assembly 生命周期与热卸载（见 [017 编译产物、包体系与类型身份](017-build-artifacts-packages.md)）。二者**不合并**，但共享 `rt_library` 注册表；`runtime`/`auto` 模块经 `rt_library_load` 加载即自动获得代数与 tombstone 语义，卸载保护继承。

## 边界

- 本篇只讲**`.ani` 契约、符号验证、marshal、加载模型与回调**；内存模型见 [005 内存模型与资源安全](005-memory-model.md)。
- FFI 装箱 ABI（`rt_box_*`）符号面见 [014 运行时 ABI](014-runtime-abi.md)。
- 动态库 Entry / AssemblyLoadContext / 热卸载见 [017 编译产物、包体系与类型身份](017-build-artifacts-packages.md)。

---
上一节：[015 LLVM 原生后端](015-llvm-backend.md) · 下一节：[017 编译产物、包体系与类型身份](017-build-artifacts-packages.md)