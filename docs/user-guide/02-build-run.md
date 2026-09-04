# 02 构建与运行

本章介绍 Arc 项目的构建与运行：`arc build` / `arc run` 的用法、输出目录约定与项目结构。

## 项目结构与目录约定

Arc 编译器使用两个固定目录承载产物，不提供配置项：

| 路径 | 用途 |
|------|------|
| `<project_root>/obj/` | 中间产物（`out.c`、`out.o`、生成的 `.g.as` 等） |
| `<project_root>/bin/` | 最终可执行文件 |

子产物（如 QIF 质检产物）有各自的 `output` 字段，缺省时从固定 `obj/` 派生（如 `obj/qif`）；显式指定时从默认中「剥离」出来，使用用户指定路径。

## arc build

对标 `dotnet build`，完整编译并链接原生二进制。`PROJECT` 可为 `.as` 文件、项目目录或 `arc.toml` 路径；缺省使用当前目录。

```bash
# 单文件构建（Debug 默认）
arc build Program.as -o hello.exe

# Release 构建 + 指定运行时
arc build Program.as -c Release -r x86_64-unknown-linux-gnu -o hello

# 项目目录模式（自动查找 arc.toml 和入口文件）
arc build .

# 项目目录模式 + --project 显式指定
arc build --project ./examples/CompilerSmoke
```

常用选项：

| 选项 | 含义 |
|------|------|
| `PROJECT`（位置） | 项目路径（`.as` 文件或目录），默认 `.` |
| `--project <PATH>` | 与 `PROJECT` 等价 |
| `-c`, `--configuration <CONFIG>` | 构建配置：`Debug`（默认）或 `Release` |
| `-r`, `--runtime <RUNTIME>` | 目标运行时标识（host triple）；亦可用 `--target` |
| `-o`, `--output <OUTPUT_DIR>` | 输出目录（默认 `bin/<config>/`） |
| `--verbosity <LEVEL>` | 日志级别：`quiet` / `minimal` / `normal`（默认）/ `detailed` / `diagnostic` |
| `-g`, `--debug` | 发射 DWARF 5 调试信息 |
| `--dynamic` | 编译为动态库（`.dll`/`.so`/`.dylib`） |
| `--ani-native-lib <DIR>` | Native 库搜索路径（可重复） |
| `--obj-dir <DIR>` | 中间产物目录（覆盖 `obj/<config>/`） |

## arc run

对标 `dotnet run`，编译后执行。`PROJECT` 可为 `.as` 文件或项目目录。

```bash
arc run Program.as
arc run Program.as -c Release
arc run Program.as --no-build    # 跳过编译，直接运行已有二进制
```

常用选项：

| 选项 | 含义 |
|------|------|
| `PROJECT`（位置） | 项目路径（`.as` 文件或目录） |
| `--project <PATH>` | 与 `PROJECT` 等价 |
| `-c`, `--configuration <CONFIG>` | 构建配置 | 
| `-r`, `--runtime <RUNTIME>` | 目标运行时标识；亦可用 `--target` |
| `--no-build` | 跳过编译，直接运行已有二进制 |
| `--verbosity <LEVEL>` | 日志级别 |
| `-g`, `--debug` | 发射 DWARF 5 调试信息 |
| `--panic-format <FMT>` | Panic 输出格式：`human`（默认）或 `json` |

## 源文件约定

- 路径指向 `.as` 文件
- 编码 UTF-8
- 入口为 `void Main()` 或 `async Task<void> main()`

## 常见项目布局

```
MyApp/
├── arc.toml          # 项目清单（权威配置）
├── Program.as        # 入口文件
├── native/           # 自定义 Native 契约（.ani）
├── obj/              # 中间产物（编译器固定）
└── bin/              # 最终可执行文件（编译器固定）
```

## 下一步

- [16 编译器 CLI](16-compiler-cli.md) 查看 `arc` 全部子命令（含 `publish` / `test` / `inspect`）
- [17 arc.toml 项目清单](17-arc-toml-reference.md) 了解项目清单配置

---

上一节：[01 安装与快速开始](01-getting-started.md) · 下一节：[03 编码与语法标准](03-encoding-standard.md)