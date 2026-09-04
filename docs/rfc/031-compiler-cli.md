# RFC 031 编译器 CLI 与构建

> **注（2026-08-29）**：原 `crates/arc-integration` 已退场（a2627a0f）。本文所引
> `cargo test -p arc-integration ...` 验证命令与 `crates/arc-integration/tests/` 路径
> 不再可用；现行验证矩阵为 `cargo test --workspace`（运行时面
> `cargo test -p arc-tests --features full-rt`），详见仓库根 `CHANGELOG.md`。

## 背景

`arc` 是 Arc 的 AOT 编译器命令行工具（由 `crates/arc` 构建）。CLI 命令与参数全面对标 .NET CLI，使用体验一致，仅内部实现不同。`arc.toml` 是 Arc 项目的权威配置文件——单一可信来源，字段定义以章节 [17 arc.toml 项目清单](../user-guide/17-arc-toml-reference.md) 为最终权威，本 RFC 仅记录决策动机。

工具链目标：确定性输出、`target` 膨胀可控。依赖遵循**源码打包**原则（见 [017](017-build-artifacts-packages.md)）：`path` 源码引用合并进单一编译单元，全静态链接输出单 exe；`--dynamic` 产出动态库（ALC 动态加载为完整保留的核心能力，见 [017](017-build-artifacts-packages.md)）。服务期产物（`.arcgr` 语义索引、`.xml` 文档注释）从源码直接生成，供 AI 工具链消费（见 [034](034-ai-toolchain-arcgr.md)）。

## 设计决策

### 1. 子命令总览

| 命令 | 对标 | 作用 | 产物 |
|------|------|------|------|
| `arc parse <file>` | — | 词法 + 语法分析，打印 AST Debug 表示（语法调试与 Agent 验证解析） | 终端 AST |
| `arc check <file>` | — | 运行 typeck 与 borrowck，不生成代码（CI 与 Agent 快速反馈首选） | 诊断 |
| `arc build [PROJECT]` | `dotnet build` | 完整编译并链接原生二进制 | `bin/<config>/` |
| `arc run [PROJECT]` | `dotnet run` | 编译后执行；`--no-build` 可跳过编译 | 运行 |
| `arc test [PROJECT]` | `dotnet test` | 扫描 `[Fact]`/`[Theory]`，合成测试入口并运行（见 [032](032-qif.md)） | 报告 |
| `arc inspect <file>` | — | 输出语义索引可达性摘要（见 [034](034-ai-toolchain-arcgr.md)） | 终端/JSON |
| `arc env` | `go env` | 打印当前 SDK 根、std/rt/native 路径、rt_cache 与 std 解析链（见 §10） | 终端/JSON |
| `arc doctor` | `rustup doctor` | 运行 SDK/工具链环境自检（clang/MSVC/native DLL/rt_cache），有 FAIL 即非零退出（见 §10） | 报告 |
| `arc toolchain` | `rustup toolchain` | 按需安装/管理外部工具链（首个：`llvm`；见 §11） | 工具链目录 |
| `arc component` | — | 按需组件管理（首个：`wgpu`；见 §12） | 组件目录 |
| `arc release` | `rustup` 发布工具 | 发布端点协议工具：签名发布 manifest 生成 / 校验 / 密钥生成（见 §13） | `manifest.json` + `.sig` |
| `arc self-update` | `rustup update` | 签名发布自更新：验签 → 下载校验 → staging → 原子提交 → 可回滚（见 §13） | 安装态更新 |
| `arc publish` | `dotnet publish` / `cargo package` | 打包项目为 `.aopkg` 源码分发包（完整性清单 + 可选签名；见 §13） | `dist/*.aopkg(+.sig)` |

`arc --version` 输出版本号（对应 `CARGO_PKG_VERSION`）。发布产物为单一 `arc` 可执行文件（命名随平台）。

### 2. `arc build`

`PROJECT` 可为 `.as` 文件、项目目录或 `arc.toml` 路径；缺省使用当前目录。也可用 `--project <PATH>` 显式指定。

| 选项 | 含义 | 默认 |
|------|------|------|
| `-c` / `--configuration <CONFIG>` | 构建配置 `Debug` / `Release` | `Debug` |
| `-r` / `--runtime <RUNTIME>` | 目标运行时标识（host triple）；亦可用 `--target` | 宿主 triple |
| `--list-targets` | 打印已知 target 三元组与构建支持状态 | — |
| `-o` / `--output <DIR>` | 输出目录 | `bin/<config>/` |
| `--verbosity <LEVEL>` | `quiet` / `minimal` / `normal` / `detailed` / `diagnostic` | `normal` |
| `-g` / `--debug` | 发射 DWARF 5 调试信息 | — |
| `--dynamic` | 编译为动态库（`.dll` / `.so` / `.dylib`） | — |
| `--ani-native-lib <DIR>` | Native 库搜索路径（可重复） | — |
| `--obj-dir <DIR>` | 中间产物目录 | `obj/<config>/` |

**WASM 门禁**：`wasm32-unknown-unknown`、`wasm32-wasip*` 与别名 `--target web` 不在 Arc 设计面内，命中即硬错误「未支持目标」，禁止 silent 当作 native 编译。WASM 链接须 runtime 子集（无 `platform.o`）。

```bash
arc build examples/CompilerSmoke/Program.as -o hello.exe          # Debug 构建
arc build examples/CompilerSmoke -c Release -r x86_64-unknown-linux-gnu -o hello
arc build .                                                         # 项目目录模式（自动找 arc.toml 与入口）
arc build --project ./examples/CompilerSmoke
```

### 3. `arc run`

| 选项 | 含义 | 默认 |
|------|------|------|
| `-c` / `--configuration <CONFIG>` | 构建配置 | `Debug` |
| `-r` / `--runtime <RUNTIME>` | 目标运行时标识 | 宿主 triple |
| `--no-build` | 跳过编译直接运行已有二进制 | — |
| `--verbosity <LEVEL>` | 日志级别 | `normal` |
| `-g` / `--debug` | 发射 DWARF 5 调试信息 | — |
| `--ani-native-lib <DIR>` | Native 库搜索路径（可重复） | — |
| `--panic-format <FMT>` | Panic 输出格式 `human` / `json` | `human` |

### 4. `obj/` 与 `bin/` 分离

`obj/` 与 `bin/` 是**编译器固定目录**，不提供配置。

| 路径 | 用途 | 派生关系 |
|------|------|---------|
| `<project_root>/obj/` | 中间产物（`out.c`、`out.o`、`.g.as` 等） | 子产物 output 默认从此派生（如 QIF 产物默认 `obj/qif`） |
| `<project_root>/bin/` | 最终可执行文件 | — |

子产物 output 配置范式：子产物有自己的 `output` 字段或 CLI 参数时，缺省从固定 `obj/` 派生；显式指定时从默认中「剥离」出来。CLI 参数 `--output` 同样适用。`--obj-dir` 仅覆盖中间产物层，不改变 `bin/` 语义。

**防回退门禁**（不得静默改回 `target/bin` / `target/obj`）：

| 门禁 | 作用 |
|------|------|
| `crates/arc-integration/tests/project_artifact_layout_e2e.rs` | 默认 `arc build` 后断言 exe∈`bin/<Config>/`、中间产物∈`obj/<Config>/`，且项目下**不得**出现 `target/bin` / `target/obj` |
| `scripts/check-project-artifact-layout.ps1`（CI） | 扫描 `crates/arc/src`，禁止 `.join("target").join("bin"\|"obj")` 等默认路径回潮 |

Cargo 工作区 `target/` 与 e2e 夹具 `target/e2e/<name>/`（可自带 `obj/`）仍合法；与**项目模型**的 `bin/`/`obj/` 分离。

### 5. 缓存

源码打包下无依赖解析与包缓存。`$ARC_HOME` 是**用户级工具链域根**（未设时 `~/.arc` / `%USERPROFILE%\.arc`），其下承载：

| 子目录 | 内容 |
|--------|------|
| `$ARC_HOME/rt_cache` | runtime C 源码编译所得 `.o` 缓存（用户数据，不随 SDK 移动；见 [017 sdk-layout](017-build-artifacts-packages/references/sdk-layout.md) §4） |
| `$ARC_HOME/tools` | 按需工具链（`llvm`）与组件（`wgpu`）落点（见 §11 / §12） |

增量构建（模块/函数级缓存、`.arcgr` 辅助增量 typeck）以 [036 §2](036-maturity.md) 的**增量构建门禁协议**为前置——无门禁不改性能面，且另立 RFC，不在此预定方向。

### 6. 构建裁剪（四层）

源码打包使四层裁剪对全部依赖成立（见 [017 源码打包](017-build-artifacts-packages.md)）：

| 层 | 内容 |
|----|------|
| 语义级 | `using` 未引用文件忽略；未实例化的泛型模板不进 codegen——单态化天然丢弃 |
| 字段级 | 从未 `load` 的结构体字段移出内存布局（相关 store 视为 NOP） |
| IR 级 | LLVM LTO 全局内联 + 死代码消除（Release；`-flto`） |
| 链接器级 | section GC：`-ffunction-sections` / `-fdata-sections` + `--gc-sections`（ELF/MinGW）/ `/OPT:REF`（MSVC） |

「拆包」由入口可达性分析（`reachability` crate，L2）完成：以 `main`（可执行）/ `Entry`（`--dynamic` 动态库，见 [017 动态库 Entry 根集可达性裁剪](017-build-artifacts-packages.md)）为根沿调用图标记可达函数/类型/字段，构成裁剪的绝对最小边界（见 讨论稿）。

### 7. `arc.toml` 设计原则

| 原则 | 内容 |
|------|------|
| 极简 | 字段最小化、缺省值合理；不复制 npm/Cargo 全功能生态 |
| 可 diff | TOML 纯文本、行友好，便于 Code Review 与 AI 编辑 |
| 单一权威 | 字段定义归章节 17，RFC 仅记录决策动机，禁止在 RFC 重复字段定义 |
| 声明性优先 | 字段尽量声明性无行为分支，复杂行为归编译器核心逻辑 |
| CLI 优先级覆盖 | 所有字段均可被 CLI 参数覆盖 |

**字段优先级链**（高 → 低）：`CLI 参数 > arc.toml 字段 > 内置默认值`。

**命名规范**（去冗余）：`[package].name` 而非 `pkg_name`；`[package].namespace` 而非 `namespace_root`；数组字段用复数（`global_usings`）。子节合并为父节加前缀，避免嵌套冗余。

**节概览**：

| 节 | 必填 | 关键字段 | 说明 |
|----|------|---------|------|
| `[package]` | ✅ | `name`、`edition`、`version`、`kind`、`namespace`、`global_usings`、`dynamic`、`abi`、`internals_visible_to` | 包元数据；`namespace` 默认同 `name` |
| `[dependencies]` | ❌ | `path`（唯一形态） | 源码级项目引用（对标 `ProjectReference`）；递归发现传递依赖，环引用报错 |
| `[native]` | ❌ | `ani-native-lib` | 链接器库搜索路径；主程序根目录恒为隐式第一项 |
| `[ui]` | ❌ | `arml`、`sources`、`program`、`namespace` | ARML 项目源文件清单 |
| `[std]` | ❌ | `path` | std 库路径覆盖（开发调试用） |
| `[workspace]` | ❌ | `members` | 解决方案 = workspace 聚合，拓扑序一键构建 |

**默认隐式引入 `Arc`**：用户项目无须声明 `Arc` 依赖；扩展子库（`Arc.Net` / `Arc.Security` / `Arc.Orm` 等）以 `path` 引用显式声明。

**workspace 拓扑**：workspace 成员间 `path` 引用构成项目引用拓扑（`Workspace::build_order` 拓扑序，被依赖者先构建），环引用报错（对标 C# 项目引用亦禁止环）。

### 8. 源文件约定与退出码

- 路径指向 `.as` 文件，编码 UTF-8；入口为 `void Main()` 或 `async Task<void> main()`。
- **Native 契约文件**：编译器内置契约在 SDK 布局的 `lib/native/`（安装态）/ `crates/arc/native/`（仓库态，`load_native_contracts` 自动扫描，如 `libc` / `rt_library` / `rt_process`；定位规则见 [017 sdk-layout](017-build-artifacts-packages/references/sdk-layout.md)）；用户项目可建项目根 `native/` 目录放置自定义契约（同模块名覆盖内置）。`arc build` 自动扫描并加载为 `NativeModule`，用户经 `using` 引入后以 `<Module>.<fn>(...)` 调用外部 C 库。

| 退出码 | 含义 |
|--------|------|
| 0 | 成功 |
| 1 | 诊断错误或 IO/链接失败 |

### 9. 环境变量清单

工具链环境变量统一在此登记（一变量一语义；定义与完整定位规则见引用文档）：

| 变量 | 语义 | 默认 | 定义 |
|------|------|------|------|
| `ARC_SDK_ROOT` | 显式指定 SDK 根（可重定位的显式覆盖）；即使目录缺布局标记也原样返回 | 无（`current_exe()` 自定位） | [017 sdk-layout §3](017-build-artifacts-packages/references/sdk-layout.md) |
| `ARC_STD_ROOT` | 显式指定 std 库根目录（开发调试用）；优先级低于 SDK 捆绑 std、高于 `workspace/std` 兜底 | 无（SDK 捆绑 std） | [017 sdk-layout §3.3](017-build-artifacts-packages/references/sdk-layout.md) |
| `ARC_HOME` | 用户级工具链域根：runtime `.o` 缓存 `rt_cache/`、按需工具链 `tools/` | `~/.arc` / `%USERPROFILE%\.arc` | 本 RFC §5 |
| `ARC_CLANG` | clang 显式覆盖（与 `arc build` 同一解析序：`ARC_CLANG` → toolchain 指针 → 标准安装位 → PATH） | 无 | `codegen::clang_path()` |

**优先级链**：`[std].path`（项目显式覆盖）> SDK 捆绑 std > `ARC_STD_ROOT` > `workspace/std` 兜底；SDK 根定位链：`ARC_SDK_ROOT` > `current_exe()` 自定位 > 编译期 `CARGO_MANIFEST_DIR` 开发兜底。语义变更须随 [017 sdk-layout](017-build-artifacts-packages/references/sdk-layout.md) 同步。

### 10. `arc env` / `arc doctor`（工具链诊断命令）

面向 SDK 打包/安装场景新增两个诊断命令，供用户、CI 与安装器消费。

**`arc env [--json]`**（对标 `go env` / `go env -json`）：打印当前环境的完整资源解析快照：

| 键 | 语义 |
|----|------|
| `ARC_VERSION` | 编译器版本 |
| `ARC_EXE` | `current_exe()` 路径 |
| `ARC_SDK_ROOT` | SDK 根（`ARC_SDK_ROOT` 覆盖或自定位；空 = 未定位） |
| `SDK_LAYOUT` | `installed` / `repo` / `none` |
| `ARC_STD_ROOT` | **生效**的 std 根（完整解析链结果） |
| `STD_SOURCE` | std 胜出来源：`[std].path` / `sdk` / `ARC_STD_ROOT` / `workspace` |
| `MANIFEST_STD_PATH` / `ARC_STD_ROOT_ENV` | 原始值（便于复现链） |
| `ARC_RT_BASE` / `ARC_NATIVE_DIR` | runtime C 基目录 / 内置 `.ani` 契约目录 |
| `ARC_RT_CACHE` / `ARC_HOME` | 用户级缓存与工具链域根 |
| `ARC_TOOLS_ROOT` / `ARC_COMPONENTS_ROOT` | 按需工具链根 / 按需组件根（`arc toolchain` / `arc component` 落点） |
| `ARC_WORKSPACE_ROOT` / `MANIFEST_DIR` | 当前目录向上定位的 workspace / 项目 |
| `ARC_CLANG` | clang 解析结果（与 `arc build` 同一解析序） |
| `HOST_TRIPLE` | 宿主 target triple |

human 输出一行一个 `NAME="value"`（空值输出 `NAME=""`）；`--json` 输出扁平对象（BTreeMap 序，机器可消费）。

**`arc doctor [--json]`**（对标 `rustup doctor`）：逐项 PASS / WARN / FAIL + 修复提示；存在任何 FAIL 时退出码 1（CI 门禁）。检测项：

| 检测 | 语义 | 失败修复提示 |
|------|------|------|
| `sdk-root` / `sdk-structure` | SDK 根自定位 + bin/lib 布局完整性 | 设置 `ARC_SDK_ROOT` 或重装 SDK |
| `clang` | clang 二进制可用（`arc build` 硬依赖） | 安装 LLVM（clang + lld ≥ 22）或设 `ARC_CLANG` |
| `vs-msvc` | Windows msvc 宿主下 clang 能定位 MSVC CRT | 安装 VS Build Tools / 修 vswhere |
| `arc-std-root` | `ARC_STD_ROOT` 设置后须指向存在目录 | 修正或取消该变量 |
| `rt-cache-writable` | rt_cache 可写 | 修权限或改 `ARC_HOME` |
| `native-dll` / `native-dll-wgpu` | `crypto_native`（必须）/ `wgpu_native`（可选，缺失 WARN） | 重装 SDK / `scripts/fetch-*-native.ps1` |
| `env-vars` | 工具链环境变量一览（信息性） | — |

实现落点：`crates/arc/src/env.rs`（快照 + 解析链 + 格式化）与 `crates/arc/src/doctor.rs`（检测）。`ARC_SDK_ROOT` 等定位规则权威见 [017 sdk-layout](017-build-artifacts-packages/references/sdk-layout.md)，本 § 只记录命令面。

### 11. `arc toolchain`（按需工具链安装）

#### 11.1 命令面

`arc toolchain install llvm [--version <ver>] [--url <url> | --archive <zip>] [--sha256 <hex>] [--force] [--no-set-env]`：

- 来源：`--url`（真实端点）或 `--archive`（本地 zip，离线/测试）；`--sha256` 可选校验（不符拒绝安装）。
- 安装到 `<tools_root>/llvm/<ver>/`（`$ARC_HOME/tools` 或 `~/.arc/tools`），原子 rename，写 `llvm/current` 活动指针。
- **clang 联动**（单一解析序）：`codegen::clang_path()` = `ARC_CLANG` → `<tools>/llvm/current` 指针 → 标准 LLVM 安装位 → PATH。装后 `arc build` / `arc env` / `arc doctor` 自动使用。
- `--set-env`（默认）写用户环境 `ARC_CLANG`（Windows `setx` / Unix `~/.profile`）。
- **幂等**：同版本已装 → 刷新指针；`ARC_CLANG` 或 PATH 已有 clang → 提示「已就绪」并跳过（`--force` 覆盖）。
- `arc toolchain list` / `uninstall [--version <ver>]` / `status` 管理状态机（版本目录 + 指针）。

实现落点：`crates/arc/src/toolchain.rs` + `codegen::sdk_layout::{toolchain_tools_root, toolchain_llvm_dir, toolchain_llvm_clang_path}`。

#### 11.2 `arc doctor` clang 版本基线

`check_clang` 解析 `clang --version`（`clang version X.Y.Z` 行）；低于 `clang_version::LLVM_MIN_VERSION`（`22.0.0`）→ **FAIL**（判别性），版本不可解析 → PASS（跳过下限，可用性由 `clang` 项 FAIL）。基线单一来源 `clang_version.rs`，`arc doctor` / `arc toolchain` 消费同一基线。

### 12. `arc component` 与 `--bundle-llvm`（按需组件与 SDK 捆绑）

#### 12.1 `arc component`（按需组件）

| 命令 | 语义 |
|------|------|
| `arc component list` | 按组件清单列出全部组件与状态：`builtin`（随 SDK 捆绑）/ `installed <ver> (active)` / `not-installed` |
| `arc component install <name> [--version <ver>] [--url <url>] [--archive <zip>] [--sha256 <hex>] [--force]` | 下载 → SHA256 校验 → 解包 → 归一化 `bin/<os>/` → 原子落位 `<tools_root>/components/<name>/<ver>` + `current` 指针 |
| `arc component uninstall <name> [--version <ver>]` | 删版本目录；删活动版本时同步清 `current` 指针 |
| `arc component status` | 组件根 + 各组件状态与活动路径 |

- **来源顺序**：`--archive`（本地/离线）> `--url`（覆盖清单模板）> 组件清单 `url` 模板（`{version}` 替换）。
- **SHA256**：`--sha256` 恒优先；本地 `--archive` 为自备产物，仅显式 `--sha256` 校验；**网络下载应用清单固定 `sha256`（强制校验，防 MITM）**。
- **幂等**：同版本已装 → 刷新 `current` 指针并结束（`--force` 重装）。
- **builtin 组件**（`crypto`）：随 SDK 捆绑，`install`/`uninstall` 明确拒绝。
- **平台限制**：清单 `platforms` 与宿主 triple 不符 → 拒绝安装（明确报错）。
- **codegen 联动（单一解析序）**：`codegen::wgpu_native_bin_dir(target)` = 组件活动目录 `components/wgpu/<ver>/bin/<os>`（存在时）→ vendored `<rt-base>/runtime-ui/wgpu-native/bin/<os>`。装后 `arc build`（lib path 注入 / DLL 复制 / `-lwgpu_native`）/ `arc doctor` 自动使用组件。

**组件清单协议（`components.json`，内嵌于编译器二进制）**：

```json
{
  "schema_version": 1,
  "components": {
    "wgpu": {
      "description": "…",
      "version": "v29.0.1.1",
      "url": "https://github.com/gfx-rs/wgpu-native/releases/download/{version}/wgpu-windows-x86_64-msvc-release.zip",
      "sha256": "<64-hex>",
      "size": 17062968,
      "platforms": ["x86_64-pc-windows-msvc"],
      "builtin": false,
      "default": false
    },
    "crypto": { "description": "…", "builtin": true, "default": true }
  }
}
```

| 字段 | 语义 |
|------|------|
| `builtin` | 随 SDK 捆绑，不可 install/uninstall |
| `version` | 固定版本（与 `url` 模板 `{version}` 占位联动） |
| `url` | 下载 URL 模板（`builtin` 组件为缺省） |
| `sha256` / `size` | 固定校验值 / 字节数（网络下载强制 sha256） |
| `platforms` | 支持的目标 triple 子串列表（空 = 不限） |

**实现落点**：`crates/arc/src/components.rs` + `crates/arc/src/components.json` + `codegen::sdk_layout::{components_root, component_dir, component_active_dir}`。

#### 12.2 `arc-pack.ps1 -BundleLlm`（捆绑瘦身版 LLVM）

- 从 `ARC_CLANG` → 标准 LLVM 安装位（`C:\Program Files\LLVM` 等）→ PATH 定位 clang；把 clang 驱动族 + lld 链接器族 + `llvm-rc/ar/ranlib` 复制到 `<sdk>/lib/llvm/bin/`。
- **瘦身版原则**：仅 `arc build` 链路必需工具；不包含 clangd/lldb/Flang/OpenMP 等扩展工具（LLVM 官方 Windows 安装器 ~455 MB，捆绑将令 SDK 包体膨胀数十倍）。`version.txt` 记录 `llvm=bundled`；`arc.env` 模板标注 `ARC_CLANG=<sdk>/lib/llvm/bin/clang[.exe]`。
- **判别验收**：解包后设 `ARC_CLANG` 指向捆绑 clang → `arc env` 输出该值 → `arc doctor` clang 检测 PASS。

#### 12.3 平台矩阵

| 平台 | 安装形态 |
|------|----------|
| Windows x64 | `install.ps1`（zip + 用户级 PATH） |
| Windows ARM64 / Linux / macOS | `arc-install.sh`（tar.xz） |
| macOS `.pkg`、Linux deb/rpm | 官方安装器 / 发行包 |

### 13. `arc release` / `arc self-update` / `arc publish`（发布与分发）

实现落点：`crates/arc/src/{release,release_sign,self_update,publish,version}.rs`。

#### 13.1 签名发布清单协议（manifest.json + 分离签名）

发布根（`ARC_RELEASE_BASE` / `--source`）下固定两文件：

| 文件 | 内容 |
|------|------|
| `manifest.json` | 版本清单：`{schema_version, channel, created, clang_min_version, versions: {ver: {date, artifacts: {triple: {url, sha256, size}}}}}`（UTF-8 无 BOM） |
| `manifest.json.sig` | 单行 `<64-hex 公钥> <64-hex 签名>`——Ed25519 覆盖 **manifest.json 原始字节**（分离签名，无 JSON 规范化问题） |

- **信任锚**：编译期内置 `RELEASE_PUBLIC_KEY_HEX`（`arc release keygen` 生成、私钥离线托管）；`$ARC_RELEASE_PUBKEY` 显式覆盖（测试/轮换迁移期）。验签失败硬错误，禁降级跳过。
- **环境变量**：`ARC_RELEASE_BASE`（发布根基址，`--source` 覆盖）、`ARC_RELEASE_PUBKEY`（验签 pin）、`ARC_RELEASE_SIGNING_KEY`（签名 seed，64 hex）。
- `arc-pack.ps1 -Manifest` 消费同一 CLI 面（`release manifest --version --triple --archive --output [--url-prefix]`），URL 缺省按发布根前缀 + 包文件名派生。

#### 13.2 `arc release` 命令面

| 子命令 | 语义 |
|--------|------|
| `arc release keygen [--seed <hex>]` | 生成（或从 seed 派生）Ed25519 密钥对；输出 `ARC_RELEASE_SIGNING_KEY`（离线托管）与 `ARC_RELEASE_PUBKEY`（内嵌信任锚） |
| `arc release manifest --archive <包>… [--version] [--triple]… [--url] [--url-prefix] [--output] [--key]` | 从本地分发包计算 SHA256/size → 构建单版本多平台清单 → 签名写出 `manifest.json` + `.sig` |
| `arc release verify <SOURCE> [--version] [--triple] [--archive]` | 解析 → 验签 →（可选）比对指定版本分发包 SHA256/size（本地 `--archive` 或按 manifest 下载） |

发布源支持 `https://…`、`file:///…` 与本地目录三形态（测试与自托管镜像同链路）。

#### 13.3 `arc self-update`（自更新）

安装布局（与 `install.ps1` / `arc-install.sh` 三方一致）：

```text
<install_root>/                  ARC_INSTALL_ROOT > Windows %LOCALAPPDATA%\arc > ~/.arc
├── bin/arc(.exe)                稳定 PATH 指针（活动版本副本；唯一 PATH 注入点）
└── versions/
    ├── current                  活动版本标记（UTF-8 无 BOM；读取容忍 BOM）
    ├── current.previous         上一版本（--rollback 目标）
    └── arc-<ver>-<triple>/      版本目录（多版本共存；回滚即切指针）
        └── bin/arc(.exe)
```

流程：验签 manifest → 选目标（缺省 = 高于当前的最新版本且含宿主 artifact）→ 下载分发包（SHA256/size 全量比对）→ `versions/.staging-<pid>/` 解压 → staged `arc --version` 布局自检 → 原子提交（目录 rename → bin 指针 → current 标记 → previous 标记；`fs_util::rename_with_retry` 承受 AV 瞬时锁）。任一步失败删除 staging，不触碰指针与标记（无副作用原则）。

- **指针身份 re-exec**：以 `bin/arc` 指针运行时先 spawn 活动版本再立即退出（Windows 运行中 exe 不可替换）。
- `--check` 仅检查不落盘；`--rollback` 切回 `current.previous`；`--version` 精确钉版；`--force` 重装。
- 分发容器统一 zip（`archive::extract_zip` 目录穿越防御）；tar.xz 解析随 Unix 分发包产线交付补齐。

#### 13.4 `arc publish`（`.aopkg` 源码分发包）

包形态（zip 容器，顶层目录 `<name>-<version>/`）：`arc.toml` + 源码（`.as`/`.arml`，递归收集，排除 `obj/` `bin/` `target/` `dist/` `.git/`）+ `native/` 契约（全文件）+ `FILES.json`（`{schema_version, package, version, files: [{path, sha256, size}]}`，路径相对顶层目录、`/` 分隔、字典序）。

| 命令 | 语义 |
|------|------|
| `arc publish [PROJECT] [-o DIR] [--key <seed>]` | 打包到 `dist/`（缺省 `<project>/dist/`）；`$ARC_RELEASE_SIGNING_KEY` 或 `--key` 存在时产出分离签名 `<pkg>.aopkg.sig`（与发布 manifest 同一密钥/信任锚/协议），否则诚实输出未签名 |
| `arc publish --verify <PKG> [--sig <SIG>]` | 消费端校验：包内容封闭性（无清单外条目、无顶层目录外夹带）→ 逐文件 SHA256/size → 可选分离签名验签（`--sig` > 包旁 `.aopkg.sig`；无签名文件则明示仅完整性校验） |

**边界**（对齐 [017 源码打包](017-build-artifacts-packages.md)）：`.aopkg` 是源码分发包——不编译、不做依赖求解、无预编译机器码；解包后以 `[dependencies] path` 引用即参与正常构建。workspace 解决方案根不可发布（须进入成员项目）。

#### 13.5 安装脚本与实机验收

| 平台 | 形态 |
|------|------|
| Windows x64 | `install.ps1`（zip + 用户级 PATH） |
| Windows ARM64 / Linux / macOS | `arc-install.sh`（tar.xz；`--url/--sha256/--ca/--to/--no-modify-path/--force`） |

- **`--ca <cert>`**：`curl --cacert` 自定信任锚——自托管镜像 / 企业内网场景，亦是 harness 的验证通道。
- **解压加固**：先解压至临时 staging 并校验 `<pkg>/bin/arc` 存在，再原子落位 `versions/<pkg>`——破损包不污染安装根。
- **实机验收 harness**：`scripts/packaging/verify-arc-install.sh`（Linux/macOS/WSL）——stub 分发包 + 自签证书 + 本地 HTTPS（`openssl s_server -WWW`）端到端跑通安装 / sidecar 校验 / SHA256 拒绝 / `--force` / PATH 注入 / 破损包防御五组用例；1.0 交付时 WSL2 Ubuntu 实测 10/10 通过，CI 可重复执行。

## 边界

- **源码打包、动态库、跨库类型身份与热卸载**见 [017](017-build-artifacts-packages.md)；本 RFC 只讲 CLI、目录分离与裁剪。
- **SDK 目录布局契约与资源自定位**（`bin/` + `lib/{std,rt,native}`、双布局、`current_exe()` 自定位、std 解析链、`rt_cache`）见 [017 sdk-layout](017-build-artifacts-packages/references/sdk-layout.md)；本 RFC 只登记环境变量清单与缓存域。
- **`arc test` 与 QIF** 见 [032](032-qif.md)。
- **LSP 服务**见 [033](033-lsp.md)。
- **`.arcgr` 语义产物与 AI 工具链消费**见 [034](034-ai-toolchain-arcgr.md)。
- **调试器与 MIR 解释器**见 [035](035-debugger.md)。
- **成熟度治理与宣称纪律**见 [036](036-maturity.md)。
- **发布与分发（release manifest / self-update / `.aopkg` 源码分发包）**见本 RFC §13；依赖求解体系维持裁撤（[017](017-build-artifacts-packages.md)）。
- **`arc.toml` 完整字段定义**以章节 [17 arc.toml 项目清单](../user-guide/17-arc-toml-reference.md) 为权威（`crates/arc/src/manifest.rs` 为参考实现）。

---

上一节：[030 Protobuf 二进制序列化](030-protobuf.md) · 下一节：[032 质检框架 QIF](032-qif.md)
