# 16 编译器 CLI

`arc` 是 Arc 的 AOT 编译器命令行工具。CLI 命令与参数全面对标 [.NET CLI](https://learn.microsoft.com/en-us/dotnet/core/tools/)，使用体验一致，仅内部实现不同。

## 安装与调用

开源阶段通过 Cargo 调用（发布产物为单一 `arc` 可执行文件）：

```bash
cargo build --release
cargo run -p arc -- --version
```

## 子命令

### `arc --version`

输出版本号。

### `arc parse <file>`

词法 + 语法分析，打印 AST Debug 表示。用于语法调试与 Agent 验证解析结果。

```bash
arc parse examples/CompilerSmoke/Program.as
```

### `arc check <file>`

运行 typeck 与 borrowck，不生成代码。CI 与 Agent 快速反馈的首选。

```bash
arc check examples/CompilerSmoke/Program.as
```

### `arc build [PROJECT]`

对标 `dotnet build`。完整编译并链接原生二进制。`PROJECT` 可为 `.as` 文件、项目目录或 `arc.toml` 路径；缺省使用当前目录。也可通过 `--project <PATH>` 显式指定。

| 选项 | 含义 | .NET 对标 |
|------|------|-----------|
| `PROJECT`（位置） | 项目路径（`.as` 文件或目录），默认 `.` | `dotnet build <PROJECT>` |
| `--project <PATH>` | 与 `PROJECT` 等价 | `dotnet build --project <PATH>` |
| `-c`, `--configuration <CONFIG>` | 构建配置：`Debug`（默认）或 `Release` | `dotnet build -c Release` |
| `-r`, `--runtime <RUNTIME>` | 目标运行时标识（host triple）；亦可用 `--target` | `dotnet build -r <RUNTIME>` |
| `--list-targets` | 打印已知 target 三元组与 build 支持状态 | — |
| `-o`, `--output <OUTPUT_DIR>` | 输出目录（默认 `bin/<config>/`） | `dotnet build -o <OUTPUT_DIR>` |
| `--verbosity <LEVEL>` | 日志级别：`quiet` / `minimal` / `normal`（默认）/ `detailed` / `diagnostic` | `dotnet build --verbosity` |
| `-g`, `--debug` | 发射 DWARF 5 调试信息 | — |
| `--dynamic` | 编译为动态库（`.dll`/`.so`/`.dylib`） | — |
| `--ani-native-lib <DIR>` | Native 库搜索路径（可重复） | — |
| `--obj-dir <DIR>` | 中间产物目录（覆盖 `obj/<config>/`） | — |

```bash
# Debug 构建（默认）
arc build examples/CompilerSmoke/Program.as -o hello.exe

# Release 构建 + 指定运行时（示例三元组 = Linux 宿主）
arc build examples/CompilerSmoke/Program.as -c Release -r x86_64-unknown-linux-gnu -o hello

# 项目目录模式（自动查找 arc.toml 和入口文件）
arc build .

# 项目目录模式 + --project 显式指定
arc build --project ./examples/CompilerSmoke
```

> `-r/--target` 目标三元组须为宿主桌面平台（win/linux/mac）；交叉编译管线未实现（现状见 [11 编译模型](11-compilation-model.md)）。

### `arc run [PROJECT]`

对标 `dotnet run`。编译后执行。`PROJECT` 可为 `.as` 文件或项目目录。

| 选项 | 含义 | .NET 对标 |
|------|------|-----------|
| `PROJECT`（位置） | 项目路径（`.as` 文件或目录） | `dotnet run <PROJECT>` |
| `--project <PATH>` | 与 `PROJECT` 等价 | `dotnet run --project <PATH>` |
| `-c`, `--configuration <CONFIG>` | 构建配置：`Debug`（默认）或 `Release` | `dotnet run -c Release` |
| `-r`, `--runtime <RUNTIME>` | 目标运行时标识；亦可用 `--target` | `dotnet run -r <RUNTIME>` |
| `--no-build` | 跳过编译，直接运行已有二进制 | `dotnet run --no-build` |
| `--verbosity <LEVEL>` | 日志级别 | `dotnet run --verbosity` |
| `-g`, `--debug` | 发射 DWARF 5 调试信息 | — |
| `--ani-native-lib <DIR>` | Native 库搜索路径（可重复） | — |
| `--panic-format <FMT>` | Panic 输出格式：`human`（默认）或 `json` | — |

```bash
arc run examples/CompilerSmoke/Program.as
arc run examples/CompilerSmoke/Program.as -c Release
arc run examples/CompilerSmoke/Program.as --no-build
```

### `arc test [PROJECT]`

对标 `dotnet test`。扫描源码 AST 收集 `[Fact]`/`[Theory]` 测试方法，自动生成测试宿主入口，编译为可执行文件并运行。`PROJECT` 可为 `.as` 文件或项目目录。

| 选项 | 含义 | .NET 对标 |
|------|------|-----------|
| `PROJECT`（位置） | 测试项目路径 | `dotnet test <PROJECT>` |
| `--project <PATH>` | 与 `PROJECT` 等价 | `dotnet test --project <PATH>` |
| `-c`, `--configuration <CONFIG>` | 构建配置：`Debug`（默认）或 `Release` | `dotnet test -c Release` |
| `-r`, `--runtime <RUNTIME>` | 目标运行时标识；亦可用 `--target` | `dotnet test -r <RUNTIME>` |
| `-o`, `--output <OUTPUT_DIR>` | 输出目录（默认 `bin/<config>/`） | `dotnet test -o <OUTPUT_DIR>` |
| `--no-build` | 跳过编译，直接运行已有测试二进制 | `dotnet test --no-build` |
| `--verbosity <LEVEL>` | 日志级别 | `dotnet test --verbosity` |
| `--filter <EXPRESSION>` | 过滤测试（全限定名 contains 匹配） | `dotnet test --filter` |
| `--list-tests` | 列出所有测试（不执行） | `dotnet test --list-tests` |
| `--logger <LOGGER>` | 输出格式：`human`（默认）/ `json` / `junit` | `dotnet test --logger` |
| `-g`, `--debug` | 发射 DWARF 5 调试信息 | — |
| `--ani-native-lib <DIR>` | Native 库搜索路径（可重复） | — |
| `--obj-dir <DIR>` | 中间产物目录（覆盖 `obj/<config>/`） | — |

```bash
# 运行项目所有测试
arc test examples/UnitTest

# Release 模式 + 过滤
arc test examples/UnitTest -c Release --filter "AssertTests::Equal"

# 列出测试（不执行）
arc test examples/UnitTest --list-tests

# 跳过编译直接运行
arc test examples/UnitTest --no-build
```

### `arc inspect <file> [--format human|json] [--emit PATH]`

输出语义索引可达性摘要（源码模式：运行 parse → hir → typeck → collect_arcgr_file）。

```bash
arc inspect examples/CompilerSmoke/Program.as

# 输出 JSON 并落盘 .arcgr
arc inspect examples/CompilerSmoke/Program.as --format json --emit hello.arcgr
```

### `arc env [--json]`

打印当前 SDK/工具链环境解析快照（对标 `go env`）：SDK 根、std/rt/native 路径、rt_cache、std 解析链胜出来源与 clang 解析结果。供诊断与 CI 消费。

```bash
# human（一行一个 NAME="value"）
arc env

# 机器可消费 JSON
arc env --json
```

### `arc doctor [--json]`

运行 SDK/工具链环境自检（对标 `rustup doctor`）：SDK 完整性、clang/LLVM 可用性、**clang 版本基线（LLVM 22，低于即 FAIL）**、`ARC_STD_ROOT` 一致性、rt_cache 可写、native DLL（crypto_native/wgpu_native）、MSVC（Windows msvc 宿主）与环境变量。每项 PASS/WARN/FAIL + 修复提示；存在任何 FAIL 时退出码非零。

```bash
arc doctor        # 自检报告
arc doctor --json # JSON 报告（CI 门禁）
```

### `arc toolchain install|list|uninstall|status`

按需安装外部工具链（对标 `rustup toolchain`）。首个组件 `llvm`：安装到 `$ARC_HOME/tools/llvm/<ver>`（或 `~/.arc/tools`），写 `llvm/current` 指针并接线 `ARC_CLANG`；安装后 `arc build`/`arc env`/`arc doctor` 自动使用该 clang。

```bash
arc toolchain install llvm --archive llvm-22.1.8.zip --sha256 <64-hex>  # 本地/离线
arc toolchain install llvm --url https://…/clang-22.zip                  # 真实端点
arc toolchain list              # 已装组件 + active + clang 基线
arc toolchain uninstall llvm --version 22.1.8
arc toolchain status            # 工具根 / 活动版本 / clang 解析
```

> `arc toolchain` / doctor clang 基线详见 [031 §11](../rfc/031-compiler-cli.md)。

> `arc env` / `arc doctor` 定位规则与打包/安装脚本详见 [017 sdk-layout](../rfc/017-build-artifacts-packages/references/sdk-layout.md) 与 [031 §10](../rfc/031-compiler-cli.md)。

## 参数对标速查

| .NET CLI | Arc CLI | 适用命令 |
|----------|---------|----------|
| `dotnet build <PROJECT>` | `arc build [PROJECT]` | Build |
| `dotnet run <PROJECT>` | `arc run <PROJECT>` | Run |
| `dotnet test <PROJECT>` | `arc test <PROJECT>` | Test |
| `-c\|--configuration <CONFIG>` | `-c\|--configuration <CONFIG>` | Build / Run / Test |
| `-r\|--runtime <RUNTIME>` | `-r\|--runtime <RUNTIME>`（亦可用 `--target`） | Build / Run / Test |
| `--project <PATH>` | `--project <PATH>` | Build / Run / Test |
| `-o\|--output <OUTPUT_DIR>` | `-o\|--output <OUTPUT_DIR>` | Build / Test |
| `--no-build` | `--no-build` | Run / Test |
| `--verbosity <LEVEL>` | `--verbosity <LEVEL>` | Build / Run / Test |
| `--filter <EXPRESSION>` | `--filter <EXPRESSION>` | Test |
| `--list-tests` | `--list-tests` | Test |
| `--logger <LOGGER>` | `--logger <LOGGER>` | Test |

## 源文件约定

- 路径指向 `.as` 文件
- 编码 UTF-8
- 入口为 `void Main()` 或 `async Task<void> main()`
- **Native 契约文件**：编译器内置契约自动扫描（如 `libc`/`rt_library`/`rt_process` 等）；用户项目可在项目根建 `native/` 目录放置自定义契约（同模块名覆盖内置）。`arc build` 自动扫描并加载为 `NativeModule`；用户代码通过 `using` 引入后以 `<Module>.<fn>(...)` 形式调用外部 C 库

## 退出码

| 码 | 含义 |
|----|------|
| 0 | 成功 |
| 1 | 诊断错误或 IO/链接失败 |

## 示例速查

```bash
# CompilerSmoke（Debug）
arc build examples/CompilerSmoke/Program.as -o smoke.exe

# CompilerSmoke（Release + 指定运行时——示例为 Linux 宿主目标）
arc build examples/CompilerSmoke -c Release -r x86_64-unknown-linux-gnu -o smoke

# ARML 窗口示例
arc build examples/ArmlDemo

# 运行测试
arc test examples/UnitTest
```

---

上一节：[15 能力系统](15-capability-system.md) · 下一节：[17 arc.toml 项目清单参考](17-arc-toml-reference.md)