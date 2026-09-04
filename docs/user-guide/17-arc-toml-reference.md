# 17 arc.toml 项目清单参考

`arc.toml` 是 Arc 项目的**权威配置文件**——单一可信来源（single source of truth）。本章是 `arc.toml` 的**规范性参考**，字段定义以本章为最终权威。

## 总则

### 设计原则

1. **极简**——字段最小化，缺省值合理
2. **可 diff**——TOML 格式，纯文本、行友好，便于 Code Review 与 AI 编辑
3. **单一权威**——字段定义归本章
4. **声明性优先**——字段尽量「声明性无行为分支」，复杂行为归编译器核心逻辑
5. **CLI 优先级覆盖**——所有配置字段均可被 CLI 参数覆盖（优先级链见下）

### 命名规范

**通用规则**：配置键命名必须**简洁易懂、简短表意明确、避免冗余**。

| 规则 | 正例 | 反例 | 说明 |
|------|------|------|------|
| **节名已表明意图时，字段不加节名前缀** | `[qif].output` | `[qif].qif_output` | 节名 `[qif]` 已表明意图，字段再前缀 `qif_` 属冗余 |
| **节点下仅一个 output 配置时不加 `_dir` 后缀** | `[qif].output` | `[qif].output_dir` | 节点下仅一个 output 配置时 `_dir` 后缀冗余 |
| **合并子节后字段用子节名前缀** | `[qif].log_dir` | `[qif].dir` | 子节 `[qif.log]` 合并到 `[qif]` 后，字段需 `log_` 前缀以独立表意 |
| **禁止冗余后缀** | `[package].namespace` | `[package].namespace_root` | "root" 后缀无表意价值 |
| **简短优先，但不过度缩写** | `[package].kind` | `[package].package_kind` | `kind` 在 `[package]` 上下文中已清晰 |
| **复数字段表数组** | `[package].global_usings` | `[package].global_using` | 数组类型字段用复数 |

**反模式清单**（禁止）：

- `namespace_root`、`output_directory`（冗余后缀，应为 `namespace`、`output`）
- `[qif].qif_output`、`[ui].ui_arml`（节名前缀冗余）
- `[qif].output_dir`（单一 output 配置加 `_dir` 后缀冗余，应为 `output`）
- `[qif.log]` 子节（子节嵌套冗余，应合并到 `[qif]` 节用 `log_` 前缀）
- `[build].obj_dir` / `[build].bin_dir`（编译器固定目录不应提供配置）
- `[package].pkg_name`（节名前缀 + 缩写，应为 `name`）

**子节合并原则**：节点下子节命名是冗余设计——子节名已表明意图时，字段直接放父节用前缀（如 `[qif.log].dir` → `[qif].log_dir`）。

## 完整 Schema

### `[package]` 节（必填）

包元数据。每份 `arc.toml` 必须包含此节。

| 字段 | 类型 | 必填 | 默认 | 说明 |
|------|------|------|------|------|
| `name` | string | ✅ | — | 包名（分发标识、依赖键）。库模块首段须与 `namespace` 根一致 |
| `edition` | string | ❌ | `"1"` | 语言 edition，供未来 breaking 语法版本切换 |
| `version` | string | ❌ | `"0.1.0"` | 包版本（semver） |
| `kind` | string | ❌ | `"binary"` | 包种类：`binary` / `library` |
| `namespace` | string | ❌ | 同 `name` | 命名空间根，如 `Arc` / `Arc.Net` / `Arc.Orm.SQLite` |
| `global_usings` | string[] | ❌ | `[]` | 项目级全局导入路径（合成 `global using`） |
| `dynamic` | bool | ❌ | `false` | 仅 `kind = "library"` 有效：产出动态库（`.dll`/`.so`/`.dylib`） |
| `abi` | string | ❌ | — | 动态库 ABI 声明（`"stable"`）；`dynamic = true` 时必填 |
| `capabilities` | string[] | ❌ | `[]` | 能力声明（如 `["window"]`），对齐[能力系统](15-capability-system.md) |
| `internals_visible_to` | string[] | ❌ | `[]` | 允许访问本包 `internal` 的包名列表（对标 C# `[assembly: InternalsVisibleTo]`） |

**`namespace` 字段规范**：

- **默认值**：与 `name` 一致（省略时等同 `name`）
- **子库场景**：`std/Net/arc.toml` 声明 `namespace = "Arc.Net"`，目录名 `Net` 与命名空间根 `Arc.Net` 解耦
- **统一显式声明**（std 子库规范）：std 子库无论 `name` 是否等于 `namespace` 均显式声明

**`global_usings` 字段规范**：

- **默认值**：`[]`（省略时无合成导入）
- **每项**：点分命名空间或类型路径（如 `"Arc"`、`"Arc.QIF"`）；非空、无空段
- **语义**：loader 合成 `global using` 并入编译单元并解析依赖；与项目根 `GlobalUsings.as` 可并存
- **不含别名**：别名形式仍用源码 `global using IO = Arc.IO;` 或 `GlobalUsings.as`

### 产物目录（固定）

`obj/` 与 `bin/` 是**编译器固定目录**，不提供配置。

**固定路径规则**：

| 路径 | 用途 | 派生关系 |
|------|------|---------|
| `<project_root>/obj/` | 中间产物（`out.c`、`out.o`、`.g.as` 等） | 子产物 output 默认从此派生（如 QIF 产物默认 `obj/qif`） |
| `<project_root>/bin/` | 最终可执行文件 | — |

**防回退**：默认产物**禁止**落入 `{project}/target/bin` / `{project}/target/obj`（Cargo `target/` 与 `target/e2e/` 除外）。CI 脚本 `scripts/check-project-artifact-layout.ps1` 与 L1 批量回归 `arc-tests/tests/l1_artifact_layout_batch.rs` 共同守门（见 [RFC 031 §5](../rfc/031-compiler-cli.md)）。

**子产物 output 配置范式**：子产物（如 QIF）有自己的 `output` 字段，缺省时从固定 `obj/` 派生（如 `obj/qif`）；显式指定时从默认中「剥离」出来，使用用户指定路径。CLI 参数 `--output` 同样适用此范式。

**不提供的配置**：

- 入口文件——由 CLI 参数决定（`arc build <file.as>`），不通过 manifest 字段配置
- 包根目录——由 `arc.toml` 所在目录隐式确定，无需显式字段

### `[dependencies]` 节（可选）

依赖声明（**源码打包**：依赖唯一形态为本地 `path` 源码引用，见 [RFC 017](../rfc/017-build-artifacts-packages.md)）。

| 字段形式 | 说明 |
|---------|------|
| `<name> = { path = "../compiler" }` | 本地 path 依赖（对标 `ProjectReference`），源码合并入同一编译单元 |

path 依赖递归发现传递依赖（依赖的 `arc.toml` 继续解析），环引用报错。

**默认隐式引入 `Arc`**：用户项目无须声明 `Arc` 依赖。扩展子库（`Arc.Net`/`Arc.Security`/`Arc.Orm` 等）以 `path` 引用显式声明。

### `[native]` 节（可选）

Native 契约库搜索路径。

| 字段 | 类型 | 必填 | 默认 | 说明 |
|------|------|------|------|------|
| `ani-native-lib` | string[] | ❌ | `[]` | 链接器库搜索路径（可重复），转换为 `-L<DIR>` 注入 `-l<name>` 之前。相对路径以 manifest 根目录为基准解析；**主程序根目录始终作为隐式第一项被搜索**，无需显式配置 |

**契约文件发现**：编译器内置契约自动扫描（`libc`/`rt_library` 等）；用户项目可在项目根建 `native/` 目录放置自定义契约（同模块名覆盖内置）。

**当前不提供**：`[native].contracts`——契约文件由内置契约 + 项目 `native/` 目录自动发现，无需 manifest 字段显式声明。

### Native 库解析（`.ani` 契约）

本节描述 `arc build` / `arc test` 链接期如何按平台命名约定解析 native 契约（`.ani`）对应的库文件。

#### 配置入口

native 库搜索目录通过以下三个来源汇聚：

| 来源 | 键/参数 | 说明 |
|------|---------|------|
| 主程序根目录（隐式） | — | **始终作为搜索列表第一项**。`arc build` 的项目根目录下的库文件无需任何配置即可被发现 |
| manifest | `[native].ani-native-lib` | 数组，可重复；相对路径以 manifest 根目录为基准解析为绝对路径 |
| CLI | `--ani-native-lib <DIR>` | 可重复；按用户输入原样使用 |

三者合并后去重，顺序：**主程序根目录 → manifest 路径 → CLI 路径**。

#### 平台命名约定

模块名 `<module>`（契约文件内 `native module <module>`）按目标平台映射为候选库文件名：

| 平台 | 候选库文件名（按序尝试） |
|------|-------------------------|
| Windows MSVC | `<module>.lib` |
| Windows MinGW | `lib<module>.dll.a`（import lib）/ `lib<module>.a`（static lib） |
| Linux / OHOS | `lib<module>.so` / `lib<module>.a` |
| macOS | `lib<module>.dylib` / `lib<module>.a` |

注：Windows 同时尝试 `<module>.lib`、`lib<module>.lib`、`lib<module>.dll.a`、`lib<module>.a`，以同时覆盖 MSVC 与 MinGW 工具链。

#### 搜索顺序

对每个 native 模块，库文件解析顺序：

1. **per-module 契约 `library` 目录**（最高优先）——契约内 `library = "path";` 声明的模块专属库目录
2. `ani-native-lib` 搜索列表（主程序根目录 → manifest → CLI）
3. **vendor 注入**（如 `wgpu-native` 预编译 lib 目录）
4. 系统默认路径（链接器自身搜索路径）

`libc` 永远跳过：所有平台隐式链接。

#### 符号验证流程

链接前对非 `libc` 的 native 模块执行编译期符号验证：

1. 探测符号扫描工具：Windows MSVC 优先 `dumpbin`，其他平台优先 `llvm-nm`，Unix 兜底 `nm`（工具不可用时降级为 warning，不阻断编译）
2. 按上述搜索顺序定位模块库文件；无法定位 → 跳过该模块
3. 扫描库已定义符号表，校验契约声明的每个符号存在；缺失 → 编译错误

#### 多库体系隔离与运行时路径（per-module `library`）

当第三方库的库文件散落在自身大型目录树时，可在契约内声明该模块的专属库目录，使各库体系各归其位。`library` 同时是运行时加载的**路径解析唯一主机制**，支持**两形态**（二选一）：

```text
native module browser {
    // 形态一：相对路径——相对**执行程序根目录**解析；也支持绝对路径
    library = "vendor/chromium/lib";

    // 形态二：环境变量表达式——运行时求值为绝对路径（或相对执行程序根目录的相对路径）；
    // 未设置 → Arc 语义返回空串 → 模块优雅降级（Native.IsAvailable = false）
    // library = Environment.GetEnvironmentVariable("ARC_BROWSER_LIB");

    fn launch() -> int;
}
```

- **相对路径形态**（`"vendor/chromium/lib"`）：相对**执行程序根目录**解析
- **环境变量表达式形态**（`Environment.GetEnvironmentVariable("...")`）：编译期识别固定形态（接收者 `Environment` 静态类 + `GetEnvironmentVariable(string)→string` + 参数字符串字面量）并做强类型校验；运行时求值
- **环境变量命名惯例**（建议，非强制）：`<MODULE>_LIB` 或 `<MODULE>_PATH`，统一 `ARC_` 前缀（如 `ARC_GPU_LIB` / `ARC_DB_CLIENT_PATH`）
- 该模块的库解析**优先**使用此目录（见「搜索顺序」第 1 条）；该目录同时注入为链接器 `-L<DIR>` 标志
- 不同模块可声明各自目录，互不干扰；未声明 `library` 的模块仍走 `ani-native-lib` 全局列表

**兼容性说明**：契约内 `library` 为新增可选声明，对既有 `.ani` 契约无解析影响（缺省等价于未声明）。

#### Native 源实现与同目录同名配对（`source` 与回退发现）

除链接**已编译**库（`library`）外，契约可声明**随项目编译纳入**的 C 源码（`source`，相对本 `.ani` 所在目录）；此模块符号由本地编译的 `.o` 提供，跳过外部 `-l<name>` 与外部库符号验证：

```text
native module math {
    source = "math.c";   // clang 编译 .o → 链接进产物
    fn add_c(int a, int b) -> int;
}
```

**同目录同名配对回退**：`.ani` 未声明 `source` 也未声明 `library` 时，编译器按契约所在目录查找同名词源/词库——存在同名 `.c` → 当作源实现编译；否则存在同名平台库变体 → 从该契约目录链接；两者皆无 → 走全局 `ani-native-lib` 列表 / 系统路径。显式声明优先于回退。

详细说明与完整示例见 [18 Native 组件集成](18-native-integration-guide.md)。

### `[ui]` 节（可选）

ARML 项目源文件清单。对标 WPF csproj 的 `<ApplicationDefinition>` + `<Page>` 项。

| 字段 | 类型 | 必填 | 默认 | 说明 |
|------|------|------|------|------|
| `arml` | string[] | ❌ | `[]` | ARML 文件列表（按声明顺序处理，影响 `.g.as` 生成顺序） |
| `sources` | string[] | ❌ | `[]` | 用户 partial class 源文件列表（`.arml.as`，合并到编译单元） |
| `program` | string | ❌ | — | 程序入口文件（如 `Program.as`），含 `Main()` 函数；合并到编译单元末尾 |
| `namespace` | string | ❌ | `[package].namespace` | 生成代码的命名空间 |

**编译单元合并顺序**（对标 WPF MSBuild 编译顺序）：

1. 头部：`namespace <ns>; using Arc;`
2. 每个 `arml` 生成的 `.g.as`（partial class + InitializeComponent）
3. 每个 `sources` 条目（用户 partial class 业务实现）
4. `program` 指定的入口文件（含 `Main()` 函数）

### `[std]` 节（可选，开发调试用）

std 库路径覆盖。

| 字段 | 类型 | 必填 | 默认 | 说明 |
|------|------|------|------|------|
| `path` | string | ❌ | 见解析链 | std 库根目录覆盖（开发调试用；相对 `arc.toml` 所在目录或绝对路径） |

**std 根完整解析链**（高 → 低）：

```
[std].path（项目显式覆盖） > SDK 捆绑 std > ARC_STD_ROOT 环境变量 > workspace/std 兜底
```

- **SDK 捆绑 std**：安装态 `<sdk>/lib/std`（`arc.exe` 经 `current_exe()` 自定位 SDK 根），开发态仓库 `<repo>/std`；
- **`ARC_STD_ROOT` 环境变量**：显式指定 std 库根目录（开发调试用），优先级低于 SDK 捆绑 std、高于 `workspace/std` 兜底。

### `[workspace]` 节（解决方案 = workspace 聚合）

**解决方案即 workspace，仍由 `arc.toml` 承载**——不引入独立 `.arcsln` 文件格式。workspace 根 `arc.toml` 通过 `members` 枚举若干成员项目；`arc build` / `arc check` 在 workspace 根执行时，按**依赖拓扑顺序**一键全量构建全部成员（对标 `dotnet sln build`）。每个成员必须是含自身 `[package]` `arc.toml` 的独立项目。

```toml
[workspace]
members = ["src/App", "src/Lib"]
```

| 字段 | 类型 | 必填 | 默认 | 说明 |
|------|------|------|------|------|
| `members` | string[] | ✅ | — | 成员项目路径列表（相对 workspace 根，各含自身 `arc.toml`） |

**成员间项目引用**：成员 A 引用成员 B，经 A 的 `[dependencies]` `path` 依赖指向 B 的项目根。workspace 依据该引用图产出拓扑构建顺序（被依赖者先构建），环引用报错（对标 C# 项目引用亦禁止环）。

## 字段优先级链

所有配置字段遵循统一优先级链（高 → 低）：

```
CLI 参数  >  arc.toml 字段  >  内置默认值
```

**路径字段优先级链示例**：

```
arc build --output <DIR>  >  默认 "bin/<config>/"（编译器固定目录）
arc test  --output <PATH>  >  默认 "obj/qif"（从固定 obj/ 派生）
arc test  --log-level <LEVEL> >  默认 "info"
```

**固定路径字段**（不可配置）：

```
obj/  ←  编译器固定（中间产物）
bin/  ←  编译器固定（最终可执行文件）
```

## 路径解析规则

1. **相对路径**：基于 `arc.toml` 所在目录（project root）解析
2. **绝对路径**：按系统绝对路径规则解析
3. **std 根解析链**：`[std].path`（项目显式覆盖）→ SDK 捆绑 std（`<sdk>/lib/std`，`arc.exe` 运行期自定位）→ `ARC_STD_ROOT` 环境变量 → `workspace/std` 兜底
4. **路径规范化**：编译器内部统一规范化为规范路径（`..`/`.` 展开）

## 完整示例

```toml
# arc.toml —— 完整示例（所有节）
# obj/ 与 bin/ 是编译器固定目录，不在此配置

[package]
name = "MyApp"
edition = "1"
version = "0.1.0"
kind = "binary"
namespace = "MyApp"

[dependencies]
compiler = { path = "../compiler" }

[native]
ani-native-lib = ["/usr/local/lib", "vendor/lib"]

[ui]
arml = ["App.arml", "MainWindow.arml"]
sources = ["App.arml.as", "MainWindow.arml.as"]
program = "Program.as"
namespace = "MyApp.UI"
```

## std 子库 arc.toml 规范

std 子库的 `arc.toml` 遵循以下规范：

1. **统一显式声明 `namespace`**——无论 `name` 是否等于 `namespace`，均显式声明
2. **`edition` 显式声明**——明确语言 edition
3. **`version` 省略**——std 子库版本随发行版，不独立版本化
4. **`kind` 省略**——std 子库均为 `library`（默认值），省略减少冗余

**std 子库示例**（`std/Net/arc.toml`）：

```toml
[package]
name = "Arc.Net"
edition = "1"
namespace = "Arc.Net"
```

---

上一节：[16 编译器 CLI](16-compiler-cli.md) · 下一节：[18 Native 组件集成](18-native-integration-guide.md)