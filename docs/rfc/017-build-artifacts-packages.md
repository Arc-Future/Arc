# RFC 017 编译产物、包体系与类型身份

## 背景

Arc 的构建产物与包体系围绕两个核心原则组织：

1. **源码打包（source packaging）**——依赖以**源码**形态声明（`path` 引用，对标 C# `ProjectReference`），构建时合并进单一编译单元，全静态链接 + LTO 输出单 exe。这是极致裁剪的根基：语义级（未实例化泛型直接丢弃）、字段级（未读取字段不进布局）、IR 级（LLVM LTO 全局内联）、链接器级（section GC）四层裁剪只有面对**源码**才全部成立——二进制包中泛型已实例化、字段布局已固化，语义级与字段级裁剪无从谈起。**「可达即存在，不可达即消失」**。
2. **动态加载是核心能力**——`arc build --dynamic` 产出动态库（`.dll`/`.so`/`.dylib`），跨库类型身份与热卸载由 `rt_library_*` ABI 与 `AssemblyLoadContext` 承载，对齐 C# `AssemblyLoadContext` 体验——`Load(path)` → `Entry(args)` → `is IPackage`。**永久不引入** `Invoke`/`CreateInstance`/`GetValue`/`SetValue` 动态分派，保障 AOT + 单态化性能。

### 渐进式披露（references）

本主题的能力子项以**渐进式披露**下沉至 [references/](017-build-artifacts-packages/references/)，主文档只保留架构级表述，读者/LLM 按需钻取，避免单文档膨胀。当前子项：

| 子项                                                                    | 内容                                                                                             | 关联主文档                                          |
| --------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------- |
| [SDK 布局与资源自定位](017-build-artifacts-packages/references/sdk-layout.md) | SDK 目录布局契约（`bin/` + `lib/{std,rt,native}`）、双布局等价识别、`current_exe()` 自定位、std 解析链、`rt_cache` 用户缓存 | 本节 · 边界；环境变量清单见 [031 §10](031-compiler-cli.md) |

## 设计决策

### SDK 布局与资源自定位

SDK 分发包以 `bin/` + `lib/{std,rt,native}` 组织（安装态），开发态仓库自身即 SDK（`std/` + `crates/`）；`arc.exe` 经 `current_exe()` 运行期自定位 SDK 根（Go 式 GOROOT 模式），`ARC_SDK_ROOT` 显式覆盖，取代编译期 `CARGO_MANIFEST_DIR` 固化路径。rt\_cache 落入用户级缓存 `$ARC_HOME/rt_cache`。完整目录布局契约、资源定位规则、std 解析链与验收标准见 [sdk-layout](017-build-artifacts-packages/references/sdk-layout.md)。

### 源码打包（依赖即源码）

| 原则     | 内容                                                                                    |
| ------ | ------------------------------------------------------------------------------------- |
| 唯一依赖形态 | `[dependencies]` 仅支持 `path` 源码引用（对标 `ProjectReference`）；无 version / git / registry 形态 |
| 编译模型   | 依赖源码合并进单一编译单元（同一 TU），全静态链接 + LTO 全局内联 + 死代码消除，输出单 exe                                 |
| 裁剪闭环   | 四层裁剪（语义级 / 字段级 / IR 级 / 链接器级）对源码全量成立（见 [031 §7](031-compiler-cli.md)）                 |
| 全局可见性  | 编译器对合并后 TU 内全部符号有完整类型信息；跨库类型身份仅在动态库边界由 `Entry<T>` 符号契约承载                              |
| 传递依赖   | path 依赖递归发现（依赖的 `arc.toml` 继续解析），环引用报错；无版本求解、无锁文件                                     |
| 服务期产物  | `.arcgr` 语义索引与 `.xml` 文档注释从**源码**直接生成（`arc inspect`），不随二进制分发                          |

**禁止项**：不引入预编译二进制包分发（`.ao`）、semver / MVS 版本求解、`arc.lock` 钉扎、内容寻址全局缓存、镜像源、依赖级包签名。**源码分发例外（1.0 起）**：`.aopkg` 为**源码分发包**——zip 容器承载 `arc.toml` + 源码（`.as`/`.arml`）+ `native/` 契约 + `FILES.json` 完整性清单（逐文件 SHA256），可选 Ed25519 分离签名；仅服务**分发完整性**，不编译、不做依赖求解、不承载预编译机器码——消费方解包后仍以 `path` 依赖引用。发布 manifest 签名（[031 §13](031-compiler-cli.md)）同属分发完整性设施，与依赖体系无关。裁剪承诺的前提是**不从外部动态调用未知符号**：动态加载仅经 `Entry<T>` 类型化符号契约（见下），未知符号调用不在设计面内。

### 动态库（`--dynamic`）

`arc build --dynamic` 将模块编译为共享库（`.dll`/`.so`/`.dylib`）。codegen 以 `EmitRole::DynamicLibrary` 发射：

- 内嵌调试符号表 `__arc_dbg_table` / `__arc_dbg_count`（`rt_debug.o` 硬引用，Windows PE 链接须就地解析）；

- Entry wrapper + 资源导出符号；

- 模块根元数据表 `__arc_module_roots` / `__arc_module_roots_count`（供热卸载根扫描自动发现，宿主免手动 `RegisterModuleRoot`）；

- 只读包元数据 `__arc_package_meta`（`"name\0version\0edition\0dep1\0dep2\0…\0"`，末尾为显式空终结字段形成双 NUL；`dep*` 为 `[dependencies]` 中 `path` 依赖的键，按字典序，运行时加载后校验并据此递归加载传递依赖）。

链接期机器码供给（runtime 内嵌 vs 共享）的裁决见下节「跨库符号共享策略」——dbg 表「Windows PE 链接须就地解析」的硬引用约束在共享形态下由加载期登记接替，本节符号发射面不受影响。

### 跨库符号共享策略（混合式）

**裁决状态：已裁决（2026-08-31）。** 本节裁决动态库产物的**机器码供给位置**——`__arc_package_meta` 裁决了「依赖怎么发现」，本节补上另一半：runtime 与 std 的机器码是内嵌各 dll 还是全局共享。

**问题陈述**

当前动态库产物是 Rust cdylib 式自含：每个动态库 = 自身代码 + 所用 std 的实例化部分（源码打包）+ **整个 runtime**（`EmitRole::DynamicLibrary` 链接路径将插件 `.o` + runtime `.o` 全量 + native 链入单一 dll，`rt_debug.o` 硬引用 `__arc_dbg_table`，Windows PE 链接须就地解析）。单插件场景可用，多插件 + 热卸载场景下代价随规模放大：

| 场景      | 自含式代价                                                                                                |
| ------- | ---------------------------------------------------------------------------------------------------- |
| 磁盘 / 分发 | 每个插件 dll 均携带一份完整 runtime 机器码（内存管理、集合、并发、加载器、诊断全量）                                                    |
| 进程内存    | N 个插件驻留 → N 份 runtime 副本同时映射进进程                                                                      |
| 热卸载     | ALC 多代数共存（模块代数 1..256）窗口内副本数按 代数 × 插件数 倍增：N 插件 × M 代共存 ≈ N×M 份                                       |
| ALC 正确性 | 各副本各持一份 `rt_library` 注册表与状态机状态，跨代记账无单一事实来源——**cdylib 自包含老路没法让** **`AssemblyLoadContext`（ALC）体系真正实用** |

对照 C# 模型：`coreclr.dll` 全局单副本、进程内共享（GC / JIT / 类型系统在共享运行时内），程序集 dll 只自含自身 IL 与元数据。Arc 的对应形态即下述裁决。

**裁决**

| 原则        | 裁决                                                                                                                                    |
| --------- | ------------------------------------------------------------------------------------------------------------------------------------- |
| runtime 层 | **全局单副本共享**：runtime 机器码（`rt_*.c` 编译产物）产出单一共享动态库 `arc_runtime`（`arc_runtime.dll` / `.so` / `.dylib`），宿主进程级持有，对标 C# 共享运行时 `coreclr.dll` |
| std 层     | **各 dll 自含自己实例化的部分**：std 维持源码打包，构建期合并进各编译单元，四层裁剪全量有效——对应 C# 程序集自含 IL 的形态                                                              |
| 分界判据      | 按「有无 Arc 语义面」划界：有（std / 插件代码）→ 自含 + 裁剪；无（`rt_*.c` 手写 C）→ 共享无损                                                                         |
| 卸载归属      | runtime **永不卸载**（宿主进程级持有）；`rt_library_unload_hot` 只卸载插件代数 dll                                                                         |

**禁止项**：**禁止每 dll 内嵌完整 runtime 的 cdylib 式方案**（多副本注册表状态分叉，ALC 体系无法实用）；禁止把 std 做成共享预编译二进制（泛型实例化与字段布局在二进制中固化，语义级 / 字段级裁剪失效，见相容性论证）。

**机制设计**

**符号供给**

- `arc_runtime` 导出完整 `rt_*` 冻结符号面（[014 运行时 ABI](014-runtime-abi.md)）；共享化只改变符号的**供给位置**，不改任何符号签名与语义（RFC 036 基础面冻结不受影响）。

- 插件链接时以**导入引用**替代内嵌 rt `.o`：Windows PE 经导出表 + 导入库（构建期自 `arc_runtime` 导出表生成，与 `.o` 同入 `rt_cache` 键空间）；ELF / Mach-O 经动态链接器绑定（`DT_NEEDED` / `LC_LOAD_DYLIB`）。独立共享 dll（而非宿主 exe 导出符号）是三平台统一供给形态——Mach-O 两级命名空间下 dylib 不可绑定主执行文件符号。

- 宿主形态由依赖图确定性推导：凡（传递）依赖动态库产物的宿主 exe，链接层同样改为导入引用（不再内嵌 rt 全量），进程内 runtime 单副本 = `arc_runtime`；无任何动态库依赖的项目维持全静态单 exe（背景根基不动摇）。「纯静态 exe + 动态库依赖」组合**构建期拒绝**——防双 runtime 副本静默并存；运行期 `arc_runtime` 解析失败经 `rt_library_load` 返回 NULL 呈现（既有 ABI 语义：失败不 panic）。

- 产物运行期单副本落位复用 U3 模式：项目 `bin/` 对 `arc_runtime` 经**硬链接**引用（安装态源自 `<sdk>/lib/rt/arc-runtime/` 预置位，仓库态源自 `rt_cache` 构建产物），不逐项目复制。

- `__arc_dbg_table` 归属重定位：per-module dbg 表仍由插件发射导出，`rt_debug.o` 的**链接期就地解析**硬引用改为**加载期登记**（插件加载后向共享 runtime 登记模块 dbg 表，runtime 经模块句柄寻址）——PE 导入表无法按同符号名绑定多插件多实例的约束由此消除。

- **类型元数据与 vtable 的落盘链接模型**（RFC 047 实施期实测修订）：`@.typeinfo.{T}` 维持 `linkonce_odr` + COMDAT（被 typeinfo 数组引用，落盘保留）；`@.vtable.{T}` **强定义常量**——实测无引用的 `linkonce_odr` COMDAT 节在 clang COFF 落盘时被丢弃，而 [047 透明对象图迁移](047-object-graph-migration.md)需要 `.vtable.{T}` 经 `rt_library_sym` 按 dll 解析（弱定义不可依赖）；插件 dll 为单 TU 编译，强定义无跨包重复风险，`.vtable.{T}` 与 `__arc_vtable_registry(+_count)` 经 `all_exports` 显式导出（Windows MSVC 数据符号默认不导出）。

**热卸载闭环下的归属**

- `arc_runtime` 永不卸载：仅随宿主进程退出终结；`rt_library_unload_hot` 回收序列的 `dlclose`/`FreeLibrary` 只作用于插件代数 dll。

- 弱槽 / tombstone 语义不受影响：CAS 状态机（`Active → Freezing → Unloading → Unloaded`）、ledger、模块根扫描、`rt_arc_weak_neutralize`、`E_UNLOAD_HANGING_REF` 全部实现在永不卸载的共享 runtime 内——自含式下「卸载序列实现自身随卸载被解除映射」的自指问题，共享式下天然消解。

- 单一事实来源：`rt_library` 注册表、代数表、ledger 只存在于共享 runtime 一处，多插件 × 多代数共存的跨模块记账不再有副本间分叉风险。

**内存账对比（多插件 × 多代数共存）**

设 runtime 机器码体量 R、单插件（自身代码 + 所用 std 实例化子集）体量 P、N 个插件、热替换后 M 代共存：

| 度量                   | 自含式（已禁止）  | 混合式（本裁决）          |
| -------------------- | --------- | ----------------- |
| runtime 副本数（磁盘 / 内存） | N × M     | 1（常数，与 N、M 无关）    |
| 插件代码 + std 子集副本数     | N × M     | N × M（不变——裁剪收益保留） |
| 总机器码量级               | N×M×(R+P) | R + N×M×P         |

R 含内存管理、集合、并发、加载器、诊断全量 runtime 面，与单插件 P 同量级或更大；自含式按 N×M 放大不可接受，混合式把该项收敛为常数。

**相容性论证**

- **与四层裁剪**：语义级（未实例化泛型丢弃）与字段级（未读取字段不进布局）只作用于 std / 插件源码编译单元——std 仍走源码打包，两级裁剪零变化；宿主动态形态下宿主自身代码 + std 实例化仍合并单编译单元全量 LTO，仅 rt 边界由静态变导入。runtime（`rt_*.c`）无 Arc 语义面，本就不参与语义级 / 字段级裁剪；IR 级（LTO）与链接器级（section GC）在 `arc_runtime` 自身构建期照常生效。把 std 做成共享预编译二进制则相反：泛型实例化与字段布局在二进制中固化，语义级 / 字段级裁剪失效——此即 std 不共享、runtime 共享的分界依据。

- **与** **`__arc_package_meta`** **/** **`Entry<T>`**：依赖键递归加载、Entry 符号契约、类型身份哈希均不感知符号供给位置——Entry wrapper 仍由插件导出，跨库调用仍经 `rt_library_sym` 间接调用（实现移入 `arc_runtime` 后行为不变）；meta 校验收敛到单一 runtime 副本，消除自含式下多副本校验状态分叉。

- **与 sdk-layout rt\_cache / U3 单副本机制的衔接**：`rt_cache`（运行时 `.o` 内容寻址缓存）继续服务 `arc_runtime` 的构建输入，共享 dll 构建产物按同一键空间缓存（target + config + g|nog + sanitize 变体互不混用）；安装态在 `<sdk>/lib/rt/arc-runtime/` 预置（属 **SDK 域**，版本随 SDK 绑定、随 self-update 多版本共存）。U3 vendored native dll 全局单副本（`$ARC_HOME/cache` + 硬链接，`native_cache_dir()`，UX 迭代评审 §2.3）是本裁决的**单副本先例**，属**用户域**（按内容全局唯一）——同为单副本共享形态、层级不同；落点契约见 [sdk-layout §2.1](017-build-artifacts-packages/references/sdk-layout.md)。

**迁移路径**

| 阶段  | 内容                                                                                                                                                                                 | 判据                                                 |
| --- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------- |
| 阶段一 | runtime 共享 dll 化（链接层改造）：`EmitRole::DynamicLibrary` 链接路径以导入引用替代 rt 全量内嵌；宿主 exe 动态形态同步切换；dbg 表加载期登记接替链接期就地解析                                                                         | 插件产物不含 rt 机器码（符号面 / 体积断言）；插件加载 → Entry 调用 → 卸载全链路绿 |
| 阶段二 | 验收：U5 六类用例批（并发 load/unload 竞态、同路径重载代数切换、卸载中访问弱槽、OnUnloading Cancel、传递依赖递归加载/卸载、hanging-ref 负向；见 UX iteration review (internal record)）+ 多插件内存足迹对账（N×M 场景实测对照上表，断言 runtime 副本收敛为 1） | 见下「验收协议」                                           |

实现排期不属于本文档（见 实现规划）。

**验收协议（对齐 RFC 036 宣称纪律）**

按 [036 成熟度 §4](036-maturity.md)：机制落地后须 ① U5 批全绿（六类用例 + hanging-ref 红绿论证，竞态确定性构造、flaky 即修）且 ② 多插件内存足迹对账达标，方可宣称「共享 runtime 落地 / 稳定」；任一不满足即维持未宣称状态。本文档只做设计裁决，不构成任何实现完成度的宣称。

**边界**

- 本节只裁决**机器码供给位置**（链接层），不改变既有章节语义：`Entry<T>` 符号契约（M2 确定性契约）、热卸载状态机与弱槽 / tombstone 语义、`__arc_package_meta` 递归加载、`rt_library_*` ABI 签名全部不变。

- 主程序纯静态单 exe 构建（无动态库依赖）的产物形态不变；全静态 + LTO 单 exe 仍是 `arc build` 默认形态。

- std 源码打包与四层裁剪不变；本节不引入任何预编译 std 分发形态。

- `arc_runtime` 的安装落点与定位契约见 [sdk-layout](017-build-artifacts-packages/references/sdk-layout.md)，本节不重复布局细节。

### 跨库类型身份

| 机制                                 | 语义                                                                      |
| ---------------------------------- | ----------------------------------------------------------------------- |
| 接口 `is T`                          | `rt_obj_isa` 遍历接口遍历判定跨库类型身份（M1）                                         |
| 泛型 `Entry<TParam,TResult>`         | 零装箱 codegen C ABI wrapper（M2）：`Entry(args)` 返回泛型结果，无动态分派（确定性契约见下）       |
| `AssemblyLoadContext` / `Assembly` | `Load` / `LoadByName` / `Unload` / probing paths / dependency graph（M3） |
| `__arc_package_meta` 嵌入校验          | 加载期运行时验证包元数据（M4）                                                        |

#### `Entry<T>` 泛型入口（M2 确定性契约）

`Entry<T>` 是跨库调用的**唯一强类型入口**：`Load(path)` → `Entry(args)` → 泛型结果。它把「调用点声明的 `TParam`/`TResult`」与「库导出的单态化 wrapper」经**符号名**绑定：编译期单态化（[004 类型系统](004-type-system.md)），运行时零动态分派、零装箱。以下契约补全 M2 的 codegen 间接调用机制，供实现方直接落地。

**调用点拦截**

- `Assembly.Entry<TResult>()` / `Assembly.Entry<TParameter, TResult>(TParameter?)` 是 `std/Arc/Runtime/Assembly.as` 的公开方法（已冻结 Stable 面）。codegen 在 `emit_call` 阶段按**方法身份**识别调用点（接收者静态类型 `Arc.Runtime.Assembly` + 方法名 `Entry` + 参数个数），与既有 facade 拦截链路（`Task`/`Environment`/`BitConverter` 等的 `builtin_dispatch`/`try_emit_*`，见 [014 运行时 ABI](014-runtime-abi.md)）同源。

- 拦截后降为**类型化间接调用**，分三步：

  1. 按泛型实参单态化（`Entry<double>`、`Entry<string, int>` 等，见 [004 类型系统](004-type-system.md)）计算目标符号名（见下「符号约定」）；
  2. 复用 `Assembly.ResolveSymbol`/`LookupEntry` 语义，经 `rt_library_sym(handle, symbol)` 解析函数指针（`NativePtr`）；`NULL` → `EntryPointNotFoundException`；
  3. 以**调用点声明的** **`TParam`/`TResult`** **重构的静态签名**发射 `call ptr %fn(...)` 间接调用。

- **无动态分派**：不经过 vtable / `Invoke` / 反射 / `rt_obj_isa`；间接调用是签名静态已知的裸函数指针调用，等价于 [016 验证式 FFI](016-verified-ffi.md) 中 `runtime`/`auto` `.ani` 模块的 per-module 函数指针表间接调用（\~2–5 ns/次）。

**符号约定**

wrapper 由**库侧**（`kind="library"` + `--dynamic`，`EmitRole::DynamicLibrary`）发射导出；`MainObject` 不发射。符号名把「类型身份」内联进符号，构成签名安全的基础（见下）：

| 入口形态                                         | 导出符号                                      |
| -------------------------------------------- | ----------------------------------------- |
| 无参 `Entry<TResult>()`                        | `__arc_entry__{TR}_{TR_sig}`              |
| 单参 `Entry<TParameter, TResult>(TParameter?)` | `__arc_entry_{TP}_{TR}_{TP_sig}_{TR_sig}` |

- `{TP}` / `{TR}` = 类型名的确定性 32 位身份哈希（FNV-1a，非零），与 `@.typeinfo.{T}` 的 `type_id`（`TypeId`）同源同值；同名类型恒定同 id。

- `{TP_sig}` / `{TR_sig}` = **布局指纹**（FNV-1a-64，`codegen::entry_layout_signature`）：类型的完整数据布局传递闭包——沿类继承链（基类字段在前）递归展开自定义复合类型字段，计入每层的类型名、字段偏移与字段类型；基元 / 枚举 / variant / 布局不可见类型作叶子（指纹即类型名哈希）；指纹不含字段名（重命名不改布局）与方法/属性。指纹算法单点居 codegen，宿主与插件经同一编译器编译，双端同源。**作用**：热重载换代后同名类型的布局漂移（字段增删 / 改型，含嵌套字段类型的深层变化，以及 std 等被引用类型版本不一致）从 ABI 静默错配变为加载期显式 `EntryPointNotFoundException`——同名同布局恒同指纹，同名异构必异指纹。

- `rt_library_sym(handle, name)` 按上述符号名查找（[014 运行时 ABI](014-runtime-abi.md)）；缺失返回 `NULL` → 调用点 `EntryPointNotFoundException`（库已卸载时先触发 `E_UNLOAD_HANGING_REF` 硬错误，见下「动态库加载 ABI」）。

**C ABI 编组契约（零装箱）**

wrapper 一律采用**统一单指针 C ABI** `void* → void*`（无论 Arc 层参数/返回个数与类型种类）：

| 类型种类                           | 参数编组                                                                           | 返回编组                                                      |
| ------------------------------ | ------------------------------------------------------------------------------ | --------------------------------------------------------- |
| 引用类型（`class`/`interface`）      | `ArcHeader*` 直传（零包装、零装箱）；`null` → `NULL`                                       | `ArcHeader*` 直传；返回对象进入 Arc 引用计数域，调用方照常 `rt_arc_inc`/`dec` |
| 值类型（`struct`/`enum`/`variant`） | 指向调用方已布局值的指针，wrapper 内 `memcpy` 到栈槽；`null` → 零初始化默认值（等价 `default(TParameter)`） | 指向堆分配拷贝的指针（wrapper 内分配 + `memcpy`）；调用侧一次性拥有，读取后释放         |

- 无参 `Entry<TResult>()` 的入参槽为 `%unused`（宿主传 `NULL`），仅返回 `TResult` 指针。

- **零装箱**：值类型在 Entry 边界保持**裸内存拷贝**，**不**走 [016 验证式 FFI](016-verified-ffi.md) 的 `void* ↔ object` 装箱（`rt_box_*`）路径；Entry 边界与 FFI 装箱是两条互不交叉的通道。

- 可空 `T?`：`null` 参数传 `NULL` 指针（值类型按零初始化、引用类型按空引用）；`null` 返回传 `NULL` 指针。

**签名安全**

符号名内联类型身份，使「签名不匹配」退化为「符号名不匹配」，**不可能**发生错误类型的调用：

- 库侧与调用侧各自从自身声明的类型名独立计算 `{TP}`/`{TR}`；两侧类型身份一致 ⟺ 符号名一致，此时 ABI 与调用侧重构的静态签名必然吻合。

- 类型身份不一致 → 符号名不同 → `rt_library_sym` 返回 `NULL` → **运行时** **`EntryPointNotFoundException`**（编译期不可知、运行时才加载的库）。若库的符号集在编译期已知，可复用 [016 验证式 FFI](016-verified-ffi.md) 的符号验证提前报**编译错误**。

- 符号名即身份、单一惯用法：不设运行时二次类型校验，也不做 `rt_obj_isa` 运行时类型窄化；32 位 FNV-1a 碰撞概率可忽略，视为可接受。

**边界**

- **仅** 0 参与单参两种 `Entry` 形态；多参数 `Entry`（参数个数 >1）的 C ABI 编组语义未定义，编译期拒绝。

- 无反射式调用（`Invoke`/`CreateInstance`/`GetValue`/`SetValue` 永久不引入，见背景）、无动态分派、无 varargs。

- Entry 边界不做 `object` 装箱；跨库值类型以裸内存拷贝传递，不进入 `object` 域。

- 本契约**补全**既有 M2 设计：不新增或变更 `Assembly`/`AssemblyLoadContext` 任何公开签名（`Entry<TResult>()` 与 `Entry<TParameter, TResult>(TParameter?)` 已存在且冻结，见 [036 成熟度](036-maturity.md) §3）；仅把 `Entry` 方法体从占位 `NotSupportedException` 落实为类型化间接调用。`__arc_entry_*` 由**库侧导出**，非新增 `rt_*` 宿主 ABI，基础面冻结不受影响。

### 动态库 Entry 根集可达性裁剪

**裁决状态：已裁决（2026-08-31）。** 可执行宿主按 `main` 入口做可达性裁剪；动态库同样存在入口规范——`Entry` 契约（上节）。本节把这一对称性定为裁决：动态库裁剪与可执行裁剪是**同一机制、同一调用链**（语义级裁剪单通道，见 [031 编译 CLI](031-compiler-cli.md) §6），仅根集来源不同，不设第二条裁剪路径。

| 产物形态             | 根集                                        | 无入口行为               |
| ---------------- | ----------------------------------------- | ------------------- |
| 可执行（`arc build`） | `main`（`main`/`Xxx::Main` 命名的 MIR 函数）     | ——（入口必有，编译期强制）      |
| 动态库（`--dynamic`） | Entry 契约（`Entry`/`Xxx::Entry` 命名的 MIR 函数） | 全量保留（模板剔除仍执行，见机制 5） |

**机制设计**

1. **同一过滤通道**：语义级裁剪（`filter_reachable_mir_fns`）对可执行与动态库无差别执行——入口收集按 `main`/`Entry`/`::Main`/`::Entry` 四态收根，BFS 闭包之外的 MIR 函数剔除。动态库不因产物形态豁免裁剪。
2. **Entry 本体即根**：`Xxx::Entry` 无须被库内其他代码调用即保留——其调用方在宿主侧（`Load(path)` → `Entry(args)`），在 Arc 调用图内不可达属预期形态，与可执行的 `main` 完全同构。
3. **wrapper 与裁剪的顺序耦合**：`__arc_entry_*` wrapper 由**裁剪后**的 MIR 函数名单发射（扫描裁剪后名单中 `Entry`/`Xxx::Entry` 命名函数）——凡导出的 `__arc_entry_*` 必有活函数体。「裁剪承诺的前提是不从外部动态调用未知符号」（设计决策禁令）由此在符号供给侧自动成立：裁剪不会裁掉已导出 wrapper 所依赖的函数体。
4. **强制保留面与可执行形态共用**：内置 stub、itable 槽实现、静态初始化器链均 force-keep——`__arc_module_roots` 引用的静态字段的 `__ctor` 初始化器在内，热卸载根扫描的引用面不因裁剪悬空。
5. **模板剔除先于根集判定无条件执行**（[012 编译期元编程](012-compile-time-metaprogramming.md) S6 A1）：泛型模板体引用未单态化的类型参数符号，无独立可发射的运行期 body，不因「无入口全量保留」而豁免（stub-handled 模板例外，IR 由 `emit_stubs` 直接生成）。

**边界**

- std facade 泛型方法 `Assembly.Entry` 在库内仅以泛型模板形态存在，裁剪期已按模板规则剔除，不会误入根集、不会误发射 wrapper；宿主侧调用点的 `Assembly::Entry__{TR}` 符号描述不经库侧 MIR，不参与库内根集。

- 无 `Entry` 的库走全量保留旁路，语义级裁剪不承诺；体量收敛交由四层裁剪的 IR 级/链接器级（LTO + section GC）。不为无入口库新增入口推断（禁止隐式约定，与 `Entry<T>` 显式符号契约同一纪律）。

- 跨库符号共享策略（混合式）不改变根集：runtime 共享只改机器码供给位置；std 自含部分同样接受 Entry 根集裁剪，四层裁剪在动态库形态保持有效。

**验收协议**

- 带 `Entry` 的动态库：不可达函数不出现在 IR 与产物符号面；`__arc_entry_*` 导出齐全且指向活函数体。

- 无入口动态库：模板剔除 + 全量保留行为不回归（stub-handled 模板例外保留）。

- L2 动态加载批（`Load` → `Entry` → 卸载全链路）在 Entry 根集裁剪下全绿。

### 动态库加载 ABI（`rt_library_*`）

| ABI                                                      | 语义                                                                                             |
| -------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `rt_library_load(path)`                                  | 加载动态库；`dlopen(RTLD_NOW\|RTLD_LOCAL)`/`LoadLibraryW`（UTF-8 路径）；失败返回 NULL（不 panic）               |
| `rt_library_sym(handle, name)`                           | 查找符号；失败返回 NULL                                                                                 |
| `rt_library_unload(handle)`                              | 冷卸载（单 Assembly 路径）；NULL 安全                                                                     |
| `rt_library_get_meta(handle)`                            | 读取 `__arc_package_meta`                                                                        |
| `rt_library_generation(handle)`                          | 查询模块代数（1..256；tombstone/未知返回 0）                                                                |
| `rt_library_ref_register/unregister/count(gen)`          | 跨模块外部强引用 ledger（边界变体；`rt_arc_inc/dec` 热路径零改动）                                                  |
| `rt_library_call_enter/leave(gen)`                       | 模块代码在途调用计数（Freeze 等待收敛）                                                                        |
| `rt_library_root_add/remove/scan(gen)`                   | ARC 根扫描（模块根可达闭包 + ledger 一致性复核；无全堆扫描）                                                          |
| `rt_library_weak_register/unregister/untrack(gen, slot)` | 宿主侧弱登记表：模块边界 `Weak<T>` 登记；卸载时中和（`rt_arc_weak_neutralize` 置空 target）→ 卸载后 `TryGet()` 确定性返回 NULL |
| `E_UNLOAD_HANGING_REF`                                   | 卸载后访问已卸载符号（`rt_library_sym` / `get_meta` tombstone 检测）→ `rt_panic` 硬错误，禁静默                     |

动态库与 `.ani` FFI 共享 `rt_library` 注册表；`runtime`/`auto` FFI 模块经 `rt_library_load` 加载即自动获得代数与 tombstone 语义（见 [016 验证式 FFI 与 Native 加载](016-verified-ffi.md)）。

### 热卸载（collectible ALC）

热卸载闭环为**必做**能力，对齐类 .NET 可回收 `AssemblyLoadContext`。ALC 生命周期状态机：`Active → Freezing → Unloading → Unloaded`。

`rt_library_unload_hot(handle)` 执行回收序列：**Freeze**（原子切 tombstone 预备态，拒绝新调用）→ **在途收敛**（`rt_library_call_enter/leave` 计数归零）→ **ledger 归零检测**（外部强引用归零才继续）→ **中和边界弱槽位**（`rt_arc_weak_neutralize`，`Weak<T>` 返回 NULL 不复活）→ **释放模块根** → **`dlclose`/`FreeLibrary`** → **tombstone**。返回 1=成功 / 0=悬挂拒载 / −1=在途未收敛 / −2=无效。

| 原则           | 裁决                                                                        |
| ------------ | ------------------------------------------------------------------------- |
| 根扫描          | 仅遍历模块代数可达闭包（模块根 `__arc_module_roots` + 字段 DFS），**不引入全堆扫描**；与 ledger 一致性复核 |
| 拒载           | 外部强引用非零 → 拒绝卸载并报告边界点；跨模块环靠 ledger 计数拒载（不靠收集器强回收）                          |
| `Weak<T>` 边界 | **不阻止卸载**；卸载后 `TryGet()` 确定性返回 NULL（tombstone 头语义，不悬垂、不复活）                |
| 在途调用         | 卸载前置 = 无模块代码在途执行；Freeze 等待计数归零                                            |
| 循环收集         | 模块内环由 always-on 收集器回收；跨模块环经 ledger 拒载                                     |

**宿主侧引用前置条件**（隔离探针二分实证）：根扫描闭包（模块根 + 字段 DFS）与 ledger 均不覆盖**宿主栈上的跨界普通对象引用**——宿主经 `Assembly.Entry<T>()` 持有插件对象引用时 `Unload` 仍会成功，而栈帧销毁时该引用的 `rt_arc_dec` 访问已解除映射的模块内存 → AV。**宿主必须在** **`Unload`** **前释放插件对象引用**（置 null 或使其出作用域语义等价，对标 C# 事件反订阅 / C 回调注销的跨界资源前置清理）。hanging-ref 检测的完备化（卸载时残留对象 tombstone 化、`dec` 遇 tombstone 安全跳过）为后续议题——其与「禁静默卸载」语义的张力（残留引用本应拒载）需先裁决检测边界，不在阶段一范围内。

**卸载顺序护栏（E\_UNLOAD\_DEPENDED）**：依赖边（PackageMeta.Dependencies 声明）不参与 rt 层 ledger 判定——依赖方在载时卸载被依赖方会静默成功，而依赖方后续的接口分派/实例化访问已解除映射的类型对象与代码（静默 AV 窗口）。为此 `AssemblyLoadContext.Unload` 在 OnUnloading Cancel 检查之后、rt 调用之前执行**被依赖感知预检**：按包名匹配反查在载模块的依赖声明，命中 → 回滚拒载并抛 `InvalidOperationException`（消息含 `E_UNLOAD_DEPENDED` 与依赖方名单）。配套约束：`UnloadAll` 按**依赖拓扑序**卸载（每轮挑无在载依赖方的模块），因递归依赖的登记键序≠依赖序（父模块先入表）；环依赖成员互为依赖方、护栏互锁，`UnloadAll` 终止防死循环（对齐「跨模块环经 ledger 拒载」语义）。无包元数据的模块不参与匹配（护栏盲区，由发布约定兜底）。

### 与插件内核的组合（Arc.Chord Reload）

二进制热卸载（本篇）与内核热替换（[045 D8](045-chord.md)）**正交组合**：内核负责服务面切换与失败回滚（先装新、成功后再卸旧），ALC 负责二进制代数生命周期。组合契约见 [045 D8.1](045-chord.md)。

### 类型对象根与跨库引用

模块生成的类型对象（vtable / 类型元数据根）与模块静态字段持有的 class 引用构成**模块根**。卸载释放序列：释放模块根 → 模块代数对象域触发循环收集 → 归零检测 → `dlclose`。收集失败对象清出、泄漏但不悬垂、不阻塞卸载。

### 应用上下文 AppContext（Arc.Runtime）

应用级上下文对象，对标 C# `System.AppContext` 常用子集：应用基目录、功能开关、数据槽。声明于 `std/Arc/Runtime/AppContext.as`（命名空间 `Arc.Runtime`，与 `Assembly`/`AssemblyLoadContext` 同域）。

**成员面**：

| 成员              | 签名                                                          | 语义                         |
| --------------- | ----------------------------------------------------------- | -------------------------- |
| `BaseDirectory` | `static string BaseDirectory { get; }`                      | 应用基目录；解析链见下；首触惰性缓存         |
| `SetSwitch`     | `static void SetSwitch(string name, bool isEnabled)`        | 设置/覆盖功能开关                  |
| `TryGetSwitch`  | `static bool TryGetSwitch(string name, out bool isEnabled)` | 读取开关；未定义返回 false 且输出 false |
| `SetData`       | `static void SetData(string name, object? value)`           | 设置应用数据槽（class 实例或 null）    |
| `GetData`       | `static object? GetData(string name)`                       | 读取数据槽；未定义或值为 null 均返回 null |

**`BaseDirectory`** **解析链**（首个非空即取）：

1. 当前执行程序集所在目录：`Assembly.GetExecutingAssembly()` → `Path.GetDirectoryName(asm.Name)`；
2. `ARC_BASE_DIR` 环境变量（Arc 特有扩展，.NET 无对应物）；
3. 当前工作目录：`Environment.GetCurrentDirectory()`。

首触确定后缓存（静态字段初始器，RFC 006 A3 S6a）。**不附加尾随目录分隔符**——`Path.Combine` 智能拼接（偏离 .NET 的尾随分隔符）。

**存储**：开关与数据分别由 `ConcurrentDictionary<string, bool>` / `ConcurrentDictionary<string, object>` 承载（`rt_concurrent_dict_*`，per-stripe 锁线程安全）。class 值经 codegen 在 set 时 `rt_arc_inc` 保留、get 时保留后返回（与 `Dictionary` 的 class 值语义同构）。

**与 .NET 对齐/偏离**：

- **裁剪** **`TargetFrameworkName`**：Arc 无 .NET 框架概念，无诚实值可暴露——禁假值挂面（与 `Environment` 撤下 `ProcessId`/`ProcessPath` 同纪律）。

- **类形态**：Arc 编译器当前限制 `static class` 不支持静态字段，故以普通类 + private 构造承载静态成员（对齐 `DependencyPropertyRegistry` 先例）；用户面仍为纯静态访问。

- **防御式空名**：null/空 `name` 忽略或返回未定义，不抛异常（对齐 std 既有风格，如 `AssemblyLoadContext.AddProbingPath`）。

- **数据槽值域**：值为 class 实例或 null。Arc `string` 为纯 C-string（非 `object` 子类型）；`string → object` 装箱仅接线在赋值路径、方法实参路径未接线——直接向 `SetData` 传 string 会透传原始指针，**禁止依赖**（文档化诚实差异）。

- **引用语义**：数据槽 class 值由字典持强引用，覆盖/清空不释放旧值——平台既有近似，不引入新 ABI。

**边界**：开关与数据仅进程内有效，不持久化；不做 .NET 兼容性开关的全局注册中心；不提供 `GetData` 已知槽（`FrameworkName` 等）。

## 边界

- 本篇只讲**源码打包、动态库、跨库类型身份与热卸载**；CLI 命令、目录分离与裁剪见 [031 编译器 CLI 与构建](031-compiler-cli.md)。

- **SDK 目录布局契约与资源自定位**（`bin/` + `lib/{std,rt,native}`、双布局、`current_exe()` 自定位、std 解析链、`rt_cache`）见 [sdk-layout](017-build-artifacts-packages/references/sdk-layout.md)；**环境变量清单与语义**（`ARC_SDK_ROOT` / `ARC_STD_ROOT` / `ARC_HOME`）见 [031 §10 环境变量清单](031-compiler-cli.md)。

- `.arcgr` 语义产物与 AI 工具链消费见 [034 AI 原生工具链与 .arcgr](034-ai-toolchain-arcgr.md)。

- `.ani` 契约与加载模型见 [016 验证式 FFI 与 Native 加载](016-verified-ffi.md)。

- 内存模型与引用计数见 [005 内存模型与资源安全](005-memory-model.md)。

***

上一节：[016 验证式 FFI 与 Native 加载](016-verified-ffi.md) · 下一节：[018 类型体系与反射元数据](018-type-reflection-metadata.md)
